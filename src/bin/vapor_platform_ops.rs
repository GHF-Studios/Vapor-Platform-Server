#[cfg(unix)]
mod unix_impl {
    use base64::Engine;
    use serde::{Deserialize, Serialize};
    use sha2::{Digest, Sha256};
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use sqlx::{Row, SqlitePool};
    use std::collections::HashSet;
    use std::fs::{File, OpenOptions};
    use std::io::Read;
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
    use std::path::PathBuf;
    use std::time::Duration;

    const MAX_INPUT: usize = 8192;
    const MAX_DURATION: i64 = 8 * 60 * 60;
    const CAPABILITIES: &[&str] = &["vapor_deploy", "vapor_service_control", "host_admin"];

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        operation: Option<String>,
        request_id: String,
        expected_version: Option<i64>,
        session_token: Option<String>,
        granted_capabilities: Option<Vec<String>>,
        granted_duration_seconds: Option<i64>,
        expires_at_unix: Option<i64>,
        expected_fingerprint: Option<String>,
        host_admin_confirmed: Option<bool>,
    }

    #[derive(Serialize)]
    struct Output<'a> {
        ok: bool,
        operation_id: &'a str,
        state: &'a str,
        detail: &'a str,
    }

    #[derive(Serialize)]
    struct AuthorizationRequest<'a> {
        audience: &'static str,
        session_token: &'a str,
        policy: AuthorizationPolicy,
        include_profile_id: bool,
    }

    #[derive(Serialize)]
    struct AuthorizationPolicy {
        mode: &'static str,
        requirements: [AuthorizationRequirement; 1],
    }

    #[derive(Serialize)]
    struct AuthorizationRequirement {
        kind: &'static str,
        role: &'static str,
    }

    #[derive(Deserialize)]
    struct AuthorizationDecision {
        allowed: bool,
    }

    pub async fn run_entry() {
        let args = std::env::args().skip(1).collect::<Vec<_>>();
        if args.len() == 2 && args[0] == "--expire" {
            match expire(&args[1]).await {
                Ok(()) => emit(true, &args[1], "expired", "temporary SSH grant expired"),
                Err(detail) => {
                    emit(false, &args[1], "failed", detail);
                    std::process::exit(1);
                }
            }
            return;
        }
        if !args.is_empty() {
            emit(
                false,
                "",
                "failed",
                "arguments are not accepted for interactive operations",
            );
            std::process::exit(2);
        }
        let result = run().await;
        match result {
            Ok((id, state, detail)) => emit(true, &id, state, detail),
            Err((id, detail)) => {
                emit(false, &id, "failed", detail);
                std::process::exit(1);
            }
        }
    }

    async fn run() -> Result<(String, &'static str, &'static str), (String, &'static str)> {
        if unsafe { libc::geteuid() } != 0 {
            return Err((String::new(), "root privileges required"));
        }
        let mut bytes = Vec::new();
        std::io::stdin()
            .take((MAX_INPUT + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| (String::new(), "could not read operation request"))?;
        if bytes.is_empty() || bytes.len() > MAX_INPUT {
            return Err((String::new(), "invalid operation request size"));
        }
        let input: Input = serde_json::from_slice(&bytes)
            .map_err(|_| (String::new(), "invalid operation request"))?;
        let id = uuid::Uuid::parse_str(&input.request_id)
            .map_err(|_| (String::new(), "invalid operation identifier"))?
            .to_string();
        let operation = input
            .operation
            .as_deref()
            .ok_or_else(|| (id.clone(), "missing operation"))?;
        if !matches!(
            operation,
            "dispatch_deploy" | "reconcile_deploy" | "grant_ssh" | "revoke_ssh"
        ) {
            return Err((id, "unsupported operation"));
        }
        let session = input
            .session_token
            .as_deref()
            .filter(|value| {
                (32..=256).contains(&value.len())
                    && value
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
            .ok_or_else(|| (id.clone(), "root session is required"))?;
        if !authorize_root(session).await {
            return Err((id, "root Identity authorization failed"));
        }
        let db_path = configured_value("VAPOR_PLATFORM_DB")
            .map(PathBuf::from)
            .or_else(|| {
                configured_value("VAPOR_PLATFORM_STATE")
                    .map(PathBuf::from)
                    .map(|dir| dir.join("platform.sqlite3"))
            })
            .unwrap_or_else(|| PathBuf::from("/var/lib/vapor-server/platform/platform.sqlite3"));
        let options = SqliteConnectOptions::new()
            .filename(db_path)
            .read_only(false)
            .create_if_missing(false)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .map_err(|_| (id.clone(), "Platform database is unavailable"))?;
        match operation {
            "dispatch_deploy" => {
                verify_deploy_request(&pool, &id, input.expected_version.unwrap_or(0)).await?;
                let unit = format!("vapor-deploy@{id}.service");
                run_command(
                    "/usr/bin/systemctl",
                    &["start", "--no-block", unit.as_str()],
                )
                .await
                .map_err(|_| (id.clone(), "could not queue approved deployment"))?;
                Ok((id, "queued", "approved deployment queued"))
            }
            "reconcile_deploy" => {
                reconcile_deploy(&pool, &id, input.expected_version.unwrap_or(0)).await?;
                Ok((id, "failed", "inactive deploy worker marked failed"))
            }
            "grant_ssh" => {
                grant_ssh(&pool, &id, &input).await?;
                Ok((id, "active", "temporary SSH grant activated"))
            }
            "revoke_ssh" => {
                revoke_ssh(&pool, &id, input.expected_version.unwrap_or(0)).await?;
                Ok((id, "revoked", "temporary SSH grant revoked"))
            }
            _ => Err((id, "unsupported operation")),
        }
    }

    async fn authorize_root(session_token: &str) -> bool {
        let Some(service_token) =
            configured_value("VAPOR_PLATFORM_IDENTITY_AUTHZ_TOKEN").filter(|value| {
                value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
        else {
            return false;
        };
        let endpoint = configured_value("VAPOR_PLATFORM_IDENTITY_AUTHZ_URL").unwrap_or_else(|| {
            "http://127.0.0.1:7110/api/identity/v1/internal/authorization/check".to_string()
        });
        let Ok(parsed_endpoint) = reqwest::Url::parse(&endpoint) else {
            return false;
        };
        let loopback = parsed_endpoint
            .host_str()
            .and_then(|host| host.parse::<std::net::IpAddr>().ok())
            .is_some_and(|address| address.is_loopback());
        if parsed_endpoint.scheme() != "http"
            || !loopback
            || !parsed_endpoint.username().is_empty()
            || parsed_endpoint.password().is_some()
            || parsed_endpoint.query().is_some()
            || parsed_endpoint.fragment().is_some()
            || parsed_endpoint.path() != "/api/identity/v1/internal/authorization/check"
        {
            return false;
        }
        let Ok(client) = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(1))
            .timeout(Duration::from_secs(3))
            .redirect(reqwest::redirect::Policy::none())
            .build()
        else {
            return false;
        };
        let request = AuthorizationRequest {
            audience: "vapor-platform-admin",
            session_token,
            policy: AuthorizationPolicy {
                mode: "all",
                requirements: [AuthorizationRequirement {
                    kind: "role",
                    role: "root",
                }],
            },
            include_profile_id: true,
        };
        let Ok(response) = client
            .post(endpoint)
            .bearer_auth(service_token)
            .json(&request)
            .send()
            .await
        else {
            return false;
        };
        if !response.status().is_success() {
            return false;
        }
        response
            .json::<AuthorizationDecision>()
            .await
            .is_ok_and(|decision| decision.allowed)
    }

    async fn verify_deploy_request(
        pool: &SqlitePool,
        id: &str,
        version: i64,
    ) -> Result<(), (String, &'static str)> {
        if version < 1 {
            return Err((id.to_string(), "invalid approved deploy version"));
        }
        let row = sqlx::query("SELECT status, version FROM platform_deploy_requests WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|_| (id.to_string(), "could not read deploy approval"))?;
        let Some(row) = row else {
            return Err((id.to_string(), "deploy approval does not exist"));
        };
        let status: String = row.get("status");
        let actual_version: i64 = row.get("version");
        if status != "approved" || actual_version != version {
            return Err((
                id.to_string(),
                "deploy approval is not current and approved",
            ));
        }
        Ok(())
    }

    async fn reconcile_deploy(
        pool: &SqlitePool,
        id: &str,
        version: i64,
    ) -> Result<(), (String, &'static str)> {
        if version < 1 {
            return Err((id.to_string(), "invalid executing deploy version"));
        }
        let row = sqlx::query("SELECT status, version FROM platform_deploy_requests WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|_| (id.to_string(), "could not read deploy execution"))?;
        let Some(row) = row else {
            return Err((id.to_string(), "deploy execution does not exist"));
        };
        if row.get::<String, _>("status") != "executing" || row.get::<i64, _>("version") != version
        {
            return Err((id.to_string(), "deploy execution state has changed"));
        }

        let unit = format!("vapor-deploy@{id}.service");
        let active_state = command_output(
            "/usr/bin/systemctl",
            &["show", "--property=ActiveState", "--value", unit.as_str()],
        )
        .await
        .map_err(|_| (id.to_string(), "could not inspect deploy worker state"))?;
        if !matches!(active_state.as_str(), "inactive" | "failed") {
            return Err((id.to_string(), "deploy worker is not stopped"));
        }

        let result = sqlx::query(
            "UPDATE platform_deploy_requests
             SET status = 'failed', version = version + 1,
                 result_detail = 'worker stopped before recording a result; request requires a new approval'
             WHERE id = ? AND status = 'executing' AND version = ?",
        )
        .bind(id)
        .bind(version)
        .execute(pool)
        .await
        .map_err(|_| (id.to_string(), "could not record stopped deploy worker"))?;
        if result.rows_affected() != 1 {
            return Err((
                id.to_string(),
                "deploy execution state changed during reconciliation",
            ));
        }
        Ok(())
    }

    async fn grant_ssh(
        pool: &SqlitePool,
        id: &str,
        input: &Input,
    ) -> Result<(), (String, &'static str)> {
        if !std::path::Path::new("/etc/vapor-server/ssh-access-enabled").is_file() {
            return Err((
                id.to_string(),
                "temporary SSH access is disabled on this host",
            ));
        }
        let version = input.expected_version.unwrap_or(0);
        let duration = input.granted_duration_seconds.unwrap_or(0);
        let granted = input.granted_capabilities.as_deref().unwrap_or(&[]);
        if version < 2 || !(1..=MAX_DURATION).contains(&duration) || !valid_capabilities(granted) {
            return Err((id.to_string(), "invalid grant scope or duration"));
        }
        if granted.iter().any(|capability| capability == "host_admin")
            && input.host_admin_confirmed != Some(true)
        {
            return Err((
                id.to_string(),
                "unrestricted root needs explicit confirmation",
            ));
        }
        let row = sqlx::query(
        "SELECT public_key,
         fingerprint, expires_at_unix, operation_detail, status, version FROM platform_ssh_access_requests WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|_| (id.to_string(), "could not read SSH request"))?;
        let Some(row) = row else {
            return Err((id.to_string(), "SSH request does not exist"));
        };
        let status: String = row.get("status");
        let actual_version: i64 = row.get("version");
        let fingerprint: String = row.get("fingerprint");
        let stored_expiry: Option<i64> = row.get("expires_at_unix");
        let operation_detail: Option<String> = row.get("operation_detail");
        if status != "provisioning"
            || actual_version != version
            || operation_detail.as_deref() != Some("granting")
        {
            return Err((id.to_string(), "SSH grant is not approved"));
        }
        let public_key: String = row.get("public_key");
        let expected_fingerprint = input.expected_fingerprint.as_deref().unwrap_or("");
        let computed_fingerprint = ssh_ed25519_fingerprint(&public_key)
            .ok_or((id.to_string(), "stored SSH public key is invalid"))?;
        if fingerprint != expected_fingerprint || computed_fingerprint != expected_fingerprint {
            return Err((id.to_string(), "approved SSH key changed after review"));
        }
        let expires = input.expires_at_unix.unwrap_or(0);
        if expires <= unix_now()
            || expires > unix_now().saturating_add(duration)
            || stored_expiry != Some(expires)
        {
            return Err((id.to_string(), "invalid grant expiry"));
        }
        create_managed_account(id, &public_key, granted, expires).await?;
        Ok(())
    }

    async fn create_managed_account(
        id: &str,
        public_key: &str,
        capabilities: &[String],
        expires: i64,
    ) -> Result<(), (String, &'static str)> {
        let username = ssh_username(id);
        if account_exists(&username) {
            return Err((
                id.to_string(),
                "temporary account already exists; refusing duplicate grant",
            ));
        }
        let hash = random_password_hash(id).await?;
        run_command(
            "/usr/sbin/useradd",
            &[
                "--create-home",
                "--gid",
                "vapor-temp-ssh",
                "--shell",
                "/bin/bash",
                "--password",
                &hash,
                &username,
            ],
        )
        .await
        .map_err(|_| (id.to_string(), "could not create temporary account"))?;
        let home = format!("/home/{username}");
        let ssh_dir = format!("{home}/.ssh");
        if run_command("/usr/bin/chown", &["root:root", &home])
            .await
            .is_err()
            || run_command("/usr/bin/chmod", &["0755", &home])
                .await
                .is_err()
        {
            let _ = remove_managed_account(id).await;
            return Err((id.to_string(), "could not secure temporary account home"));
        }
        let workspace = format!("{home}/workspace");
        if run_command(
            "/usr/bin/install",
            &[
                "-d",
                "-o",
                &username,
                "-g",
                "vapor-temp-ssh",
                "-m",
                "0750",
                &workspace,
            ],
        )
        .await
        .is_err()
        {
            let _ = remove_managed_account(id).await;
            return Err((id.to_string(), "could not prepare temporary workspace"));
        }
        if run_command(
            "/usr/bin/install",
            &["-d", "-o", "root", "-g", "root", "-m", "0755", &ssh_dir],
        )
        .await
        .is_err()
        {
            let _ = remove_managed_account(id).await;
            return Err((
                id.to_string(),
                "could not prepare managed SSH key directory",
            ));
        }
        let key_path = format!("{ssh_dir}/authorized_keys");
        let key_line = format!("{public_key}\n");
        if atomic_write_root_file(&key_path, key_line.as_bytes(), 0o644).is_err() {
            let _ = remove_managed_account(id).await;
            return Err((id.to_string(), "could not install managed public key"));
        }
        let sudoers_path = format!("/etc/sudoers.d/vapor-ssh-{id}");
        let sudoers = sudoers_contents(&username, capabilities);
        if atomic_write_root_file(&sudoers_path, sudoers.as_bytes(), 0o440).is_err()
            || run_command("/usr/sbin/visudo", &["-cf", &sudoers_path])
                .await
                .is_err()
        {
            let _ = remove_managed_account(id).await;
            return Err((id.to_string(), "scoped sudo policy did not validate"));
        }
        if install_expiry_units(id, expires).await.is_err() {
            let _ = remove_managed_account(id).await;
            return Err((id.to_string(), "could not install persistent expiry timer"));
        }
        Ok(())
    }

    fn ssh_ed25519_fingerprint(public_key: &str) -> Option<String> {
        let mut fields = public_key.split_whitespace();
        if fields.next()? != "ssh-ed25519" {
            return None;
        }
        let encoded = fields.next()?;
        if fields.next().is_some() {
            return None;
        }
        let blob = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(encoded))
            .ok()?;
        let mut offset = 0usize;
        let algorithm = read_ssh_string(&blob, &mut offset)?;
        let key = read_ssh_string(&blob, &mut offset)?;
        if algorithm != b"ssh-ed25519" || key.len() != 32 || offset != blob.len() {
            return None;
        }
        Some(format!(
            "SHA256:{}",
            base64::engine::general_purpose::STANDARD_NO_PAD.encode(Sha256::digest(&blob))
        ))
    }

    fn read_ssh_string<'a>(blob: &'a [u8], offset: &mut usize) -> Option<&'a [u8]> {
        let header_end = offset.checked_add(4)?;
        let header = blob.get(*offset..header_end)?;
        let length = u32::from_be_bytes(header.try_into().ok()?) as usize;
        let start = header_end;
        let end = start.checked_add(length)?;
        let value = blob.get(start..end)?;
        *offset = end;
        Some(value)
    }

    async fn revoke_ssh(
        pool: &SqlitePool,
        id: &str,
        version: i64,
    ) -> Result<(), (String, &'static str)> {
        if version < 2 {
            return Err((id.to_string(), "invalid revocation version"));
        }
        let row = sqlx::query("SELECT status, version, operation_detail FROM platform_ssh_access_requests WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|_| (id.to_string(), "could not read SSH grant"))?;
        let Some(row) = row else {
            return Err((id.to_string(), "SSH grant does not exist"));
        };
        if row.get::<String, _>("status") != "provisioning"
            || row.get::<i64, _>("version") != version
            || row.get::<Option<String>, _>("operation_detail").as_deref() != Some("revoking")
        {
            return Err((
                id.to_string(),
                "SSH grant is not at the approved revoke version",
            ));
        }
        remove_managed_account(id).await?;
        Ok(())
    }

    async fn remove_managed_account(id: &str) -> Result<(), (String, &'static str)> {
        let _cleanup_lock = acquire_cleanup_lock(id)?;
        let username = ssh_username(id);
        let key_path = format!("/home/{username}/.ssh/authorized_keys");
        let sudoers = format!("/etc/sudoers.d/vapor-ssh-{id}");
        let timer_unit = format!("vapor-ssh-expire-{id}.timer");
        let timer_path = format!("/etc/systemd/system/{timer_unit}");
        let timer_link = format!("/etc/systemd/system/timers.target.wants/{timer_unit}");
        let timer_exists = managed_path_exists(&timer_path).await.map_err(|_| {
            (
                id.to_string(),
                "could not inspect temporary access timer file",
            )
        })? || managed_path_exists(&timer_link).await.map_err(|_| {
            (
                id.to_string(),
                "could not inspect temporary access timer link",
            )
        })?;
        if timer_exists {
            run_command("/usr/bin/systemctl", &["daemon-reload"])
                .await
                .map_err(|_| {
                    (
                        id.to_string(),
                        "could not reload expiry timer before cleanup",
                    )
                })?;
            run_command("/usr/bin/systemctl", &["stop", timer_unit.as_str()])
                .await
                .map_err(|_| {
                    (
                        id.to_string(),
                        "could not stop temporary access expiry timer",
                    )
                })?;
            run_command("/usr/bin/systemctl", &["disable", timer_unit.as_str()])
                .await
                .map_err(|_| {
                    (
                        id.to_string(),
                        "could not disable temporary access expiry timer",
                    )
                })?;

            let active_state = command_output(
                "/usr/bin/systemctl",
                &[
                    "show",
                    "--property=ActiveState",
                    "--value",
                    timer_unit.as_str(),
                ],
            )
            .await
            .map_err(|_| (id.to_string(), "could not verify expiry timer state"))?;
            if !matches!(active_state.as_str(), "inactive" | "failed") {
                return Err((
                    id.to_string(),
                    "temporary access expiry timer is still active",
                ));
            }

            let unit_file_state = command_output(
                "/usr/bin/systemctl",
                &[
                    "show",
                    "--property=UnitFileState",
                    "--value",
                    timer_unit.as_str(),
                ],
            )
            .await
            .map_err(|_| (id.to_string(), "could not verify expiry timer enablement"))?;
            if unit_file_state != "disabled" {
                return Err((
                    id.to_string(),
                    "temporary access expiry timer is still enabled",
                ));
            }
        }
        for path in [
            key_path,
            sudoers,
            timer_path,
            format!("/etc/systemd/system/vapor-ssh-expire-{id}.service"),
        ] {
            match tokio::fs::remove_file(path).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => {
                    return Err((id.to_string(), "could not remove managed SSH access files"))
                }
            }
        }
        if account_exists(&username) {
            let _ = run_command("/usr/bin/pkill", &["-KILL", "-u", &username]).await;
            run_command("/usr/sbin/userdel", &["--force", "--remove", &username])
                .await
                .map_err(|_| {
                    (
                        id.to_string(),
                        "could not terminate and remove temporary account",
                    )
                })?;
        }
        run_command("/usr/bin/systemctl", &["daemon-reload"])
            .await
            .map_err(|_| (id.to_string(), "could not reload expiry units"))?;
        Ok(())
    }

    fn acquire_cleanup_lock(id: &str) -> Result<File, (String, &'static str)> {
        let lock_dir = "/run/lock/vapor-ssh-cleanup";
        match std::fs::DirBuilder::new().mode(0o700).create(lock_dir) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => {
                return Err((
                    id.to_string(),
                    "could not create temporary access cleanup lock directory",
                ))
            }
        }
        let directory_metadata = std::fs::symlink_metadata(lock_dir)
            .map_err(|_| (id.to_string(), "could not inspect cleanup lock directory"))?;
        if !directory_metadata.is_dir()
            || directory_metadata.uid() != 0
            || directory_metadata.mode() & 0o077 != 0
        {
            return Err((
                id.to_string(),
                "temporary access cleanup lock directory is unsafe",
            ));
        }
        let path = format!("{lock_dir}/grant-{id}.lock");
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
            .map_err(|_| {
                (
                    id.to_string(),
                    "could not open temporary access cleanup lock",
                )
            })?;
        let metadata = file.metadata().map_err(|_| {
            (
                id.to_string(),
                "could not inspect temporary access cleanup lock",
            )
        })?;
        if !metadata.is_file() || metadata.uid() != 0 || metadata.mode() & 0o077 != 0 {
            return Err((
                id.to_string(),
                "temporary access cleanup lock has unsafe ownership or permissions",
            ));
        }
        loop {
            let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
            if result == 0 {
                return Ok(file);
            }
            if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
                return Err((
                    id.to_string(),
                    "could not acquire temporary access cleanup lock",
                ));
            }
        }
    }

    fn ssh_username(id: &str) -> String {
        format!("vpssh_{}", id.split('-').next().unwrap_or("invalid"))
    }

    fn account_exists(username: &str) -> bool {
        std::process::Command::new("/usr/bin/id")
            .arg("-u")
            .arg(username)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    async fn managed_path_exists(path: &str) -> Result<bool, ()> {
        match tokio::fs::symlink_metadata(path).await {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(_) => Err(()),
        }
    }

    async fn random_password_hash(id: &str) -> Result<String, (String, &'static str)> {
        let mut random = [0u8; 48];
        std::fs::File::open("/dev/urandom")
            .and_then(|mut file| file.read_exact(&mut random))
            .map_err(|_| {
                (
                    id.to_string(),
                    "could not generate an isolated account password",
                )
            })?;
        let plaintext = base64::engine::general_purpose::STANDARD_NO_PAD.encode(random);
        let mut child = tokio::process::Command::new("/usr/bin/openssl")
            .args(["passwd", "-6", "-stdin"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|_| {
                (
                    id.to_string(),
                    "could not prepare a non-login password secret",
                )
            })?;
        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            stdin
                .write_all(plaintext.as_bytes())
                .await
                .map_err(|_| (id.to_string(), "password hashing failed"))?;
            stdin
                .write_all(b"\n")
                .await
                .map_err(|_| (id.to_string(), "password hashing failed"))?;
        }
        let output = child
            .wait_with_output()
            .await
            .map_err(|_| (id.to_string(), "password hashing failed"))?;
        if !output.status.success() {
            return Err((id.to_string(), "password hashing failed"));
        }
        String::from_utf8(output.stdout)
            .map(|value| value.trim().to_string())
            .map_err(|_| (id.to_string(), "password hashing returned invalid data"))
    }

    fn sudoers_contents(username: &str, capabilities: &[String]) -> String {
        if capabilities.iter().any(|value| value == "host_admin") {
            return format!("{username} ALL=(ALL:ALL) NOPASSWD: ALL\n");
        }
        let mut lines = Vec::new();
        if capabilities.iter().any(|value| value == "vapor_deploy") {
            lines.push(format!(
                "{username} ALL=(root) NOPASSWD: /usr/local/sbin/vapor-ssh-deploy"
            ));
        }
        if capabilities
            .iter()
            .any(|value| value == "vapor_service_control")
        {
            lines.push(format!(
                "{username} ALL=(root) NOPASSWD: /usr/local/sbin/vapor-ssh-service-control"
            ));
        }
        if lines.is_empty() {
            String::new()
        } else {
            format!("{}\n", lines.join("\n"))
        }
    }

    async fn install_expiry_units(id: &str, expires: i64) -> Result<(), ()> {
        let date = chrono_free_iso(expires).ok_or(())?;
        let service_path = format!("/etc/systemd/system/vapor-ssh-expire-{id}.service");
        let timer_path = format!("/etc/systemd/system/vapor-ssh-expire-{id}.timer");
        let service = format!("[Unit]\nDescription=Expire temporary Vapor SSH grant {id}\n\n[Service]\nType=oneshot\nExecStart=/usr/local/libexec/vapor-platform-ops --expire {id}\n");
        let timer = format!("[Unit]\nDescription=Expiry timer for temporary Vapor SSH grant {id}\n\n[Timer]\nOnCalendar={date}\nAccuracySec=1s\nPersistent=true\nUnit=vapor-ssh-expire-{id}.service\n\n[Install]\nWantedBy=timers.target\n");
        atomic_write_root_file(&service_path, service.as_bytes(), 0o644).map_err(|_| ())?;
        atomic_write_root_file(&timer_path, timer.as_bytes(), 0o644).map_err(|_| ())?;
        run_command("/usr/bin/systemctl", &["daemon-reload"]).await?;
        run_command(
            "/usr/bin/systemctl",
            &["enable", "--now", &format!("vapor-ssh-expire-{id}.timer")],
        )
        .await?;
        Ok(())
    }

    fn chrono_free_iso(unix: i64) -> Option<String> {
        let output = std::process::Command::new("/usr/bin/date")
            .args([
                "--utc",
                "--date",
                &format!("@{unix}"),
                "+%Y-%m-%d %H:%M:%S UTC",
            ])
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn atomic_write_root_file(path: &str, contents: &[u8], mode: u32) -> std::io::Result<()> {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let temporary = format!("{path}.{}.tmp", std::process::id());
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
        std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(mode))?;
        std::fs::rename(&temporary, path)
    }

    fn valid_capabilities(values: &[String]) -> bool {
        values.len() <= CAPABILITIES.len()
            && values
                .iter()
                .all(|value| CAPABILITIES.contains(&value.as_str()))
            && values.iter().collect::<HashSet<_>>().len() == values.len()
            && (!values.iter().any(|value| value == "host_admin") || values.len() == 1)
    }

    fn configured_value(name: &str) -> Option<String> {
        let content = std::fs::read_to_string("/etc/vapor-server/platform.env").ok()?;
        content.lines().find_map(|line| {
            let (key, value) = line.split_once('=')?;
            if key == name
                && !value
                    .chars()
                    .any(|character| matches!(character, '\n' | '\r' | '\0'))
            {
                Some(value.trim_matches('"').to_string())
            } else {
                None
            }
        })
    }

    fn platform_database_path() -> PathBuf {
        configured_value("VAPOR_PLATFORM_DB")
            .map(PathBuf::from)
            .or_else(|| {
                configured_value("VAPOR_PLATFORM_STATE")
                    .map(PathBuf::from)
                    .map(|directory| directory.join("platform.sqlite3"))
            })
            .unwrap_or_else(|| PathBuf::from("/var/lib/vapor-server/platform/platform.sqlite3"))
    }

    async fn run_command(path: &str, args: &[&str]) -> Result<(), ()> {
        let status = tokio::process::Command::new(path)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .map_err(|_| ())?;
        status.success().then_some(()).ok_or(())
    }

    async fn command_output(path: &str, args: &[&str]) -> Result<String, ()> {
        let output = tokio::process::Command::new(path)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
            .await
            .map_err(|_| ())?;
        if !output.status.success() || output.stdout.len() > 128 {
            return Err(());
        }
        let state = String::from_utf8(output.stdout).map_err(|_| ())?;
        Ok(state.trim().to_string())
    }

    async fn expire(id: &str) -> Result<(), &'static str> {
        if unsafe { libc::geteuid() } != 0 || uuid::Uuid::parse_str(id).is_err() {
            return Err("invalid expiry invocation");
        }
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(platform_database_path())
                    .create_if_missing(false),
            )
            .await
            .map_err(|_| "Platform database is unavailable")?;
        let row = sqlx::query(
            "SELECT status, expires_at_unix FROM platform_ssh_access_requests WHERE id=?",
        )
        .bind(id)
        .fetch_optional(&pool)
        .await
        .map_err(|_| "could not read SSH grant status")?;
        let Some(row) = row else {
            return Err("SSH grant does not exist");
        };
        if !matches!(
            row.get::<String, _>("status").as_str(),
            "active" | "provisioning" | "failed"
        ) || row
            .get::<Option<i64>, _>("expires_at_unix")
            .unwrap_or(i64::MAX)
            > unix_now()
        {
            return Err("SSH grant is not due to expire");
        }
        remove_managed_account(id)
            .await
            .map_err(|_| "temporary account cleanup failed")?;
        sqlx::query(
        "UPDATE platform_ssh_access_requests SET status = 'expired', version = version + 1,
         public_key = '', decided_by_profile_id = NULL, operation_detail = NULL WHERE id = ? AND status IN ('active','provisioning','failed') AND expires_at_unix <= ?",
    )
    .bind(id)
    .bind(unix_now())
    .execute(&pool)
    .await
    .map_err(|_| "could not record grant expiry")?;
        Ok(())
    }

    fn emit(ok: bool, id: &str, state: &str, detail: &str) {
        let output = Output {
            ok,
            operation_id: id,
            state,
            detail,
        };
        if let Ok(json) = serde_json::to_string(&output) {
            println!("{json}");
        }
    }

    fn unix_now() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or(0)
    }
}

#[cfg(unix)]
#[tokio::main]
async fn main() {
    unix_impl::run_entry().await;
}

#[cfg(not(unix))]
fn main() {
    eprintln!("vapor_platform_ops is a Linux server helper and cannot run on this platform");
}
