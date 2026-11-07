#[path = "../ssh_cli.rs"]
mod ssh_cli;
#[path = "../ssh_policy.rs"]
mod ssh_policy;

#[cfg_attr(not(unix), allow(dead_code))]
const MAX_IDENTITY_AUTHORIZATION_RESPONSE_BYTES: usize = 4096;

#[cfg_attr(not(unix), allow(dead_code))]
fn valid_deployment_target(value: Option<&str>) -> bool {
    matches!(value, Some("monolith" | "services"))
}

#[cfg_attr(not(unix), allow(dead_code))]
#[derive(serde::Deserialize)]
struct IdentityAuthorizationDecision {
    allowed: bool,
    expires_at_unix: i64,
}

#[cfg_attr(not(unix), allow(dead_code))]
fn identity_decision_is_current(allowed: bool, expires_at_unix: i64, now_unix: i64) -> bool {
    allowed && expires_at_unix > now_unix
}

#[cfg_attr(not(unix), allow(dead_code))]
fn identity_response_allows_root(status: reqwest::StatusCode, body: &[u8], now_unix: i64) -> bool {
    if status != reqwest::StatusCode::OK || body.len() > MAX_IDENTITY_AUTHORIZATION_RESPONSE_BYTES {
        return false;
    }
    serde_json::from_slice::<IdentityAuthorizationDecision>(body).is_ok_and(|decision| {
        identity_decision_is_current(decision.allowed, decision.expires_at_unix, now_unix)
    })
}

#[cfg_attr(not(unix), allow(dead_code))]
fn grant_setup_failure_detail(
    primary_failure: &'static str,
    rollback_completed: bool,
) -> &'static str {
    if rollback_completed {
        primary_failure
    } else {
        "grant setup failed and rollback was incomplete; access artifacts may remain; retry revoke and inspect the host"
    }
}

#[cfg_attr(not(unix), allow(dead_code))]
fn valid_identity_authz_endpoint(endpoint: &str) -> bool {
    matches!(
        endpoint,
        "http://127.0.0.1:7110/api/identity/v1/internal/authorization/check"
            | "http://127.0.0.1:7113/v1/internal/authorization/check"
    )
}

#[cfg_attr(not(unix), allow(dead_code))]
async fn record_stopped_deploy_failure(
    pool: &sqlx::SqlitePool,
    id: &str,
    version: i64,
) -> Result<(), (String, &'static str)> {
    let result = sqlx::query(
        "UPDATE platform_deploy_requests
         SET status = 'failed', dispatch_state = 'unknown', version = version + 1,
             dispatch_detail = 'worker stopped before recording a result; execution outcome is unknown',
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

    use super::ssh_policy::PENDING_TTL_SECONDS;

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

    pub async fn run_entry() {
        let args = std::env::args().skip(1).collect::<Vec<_>>();
        if args.first().is_some_and(|value| value == "ssh") {
            match super::ssh_cli::parse_ssh_args(&args[1..]) {
                Ok(super::ssh_cli::SshCommand::Help) => {
                    println!("{}", super::ssh_cli::usage());
                }
                Ok(command) => {
                    if let Err(detail) = run_ssh_command(command).await {
                        eprintln!("error: {detail}");
                        std::process::exit(1);
                    }
                }
                Err(error) => {
                    eprintln!("error: {error}\n{}", super::ssh_cli::usage());
                    std::process::exit(2);
                }
            }
            return;
        }
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

    async fn run_ssh_command(command: super::ssh_cli::SshCommand) -> Result<(), String> {
        if unsafe { libc::geteuid() } != 0 {
            return Err(
                "root privileges required; run this command through the approved operator path"
                    .to_owned(),
            );
        }
        let read_only = matches!(
            &command,
            super::ssh_cli::SshCommand::List
                | super::ssh_cli::SshCommand::Show { .. }
                | super::ssh_cli::SshCommand::Reconfigure { .. }
        );
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(platform_database_path())
                    .read_only(read_only)
                    .create_if_missing(false)
                    .foreign_keys(true)
                    .busy_timeout(Duration::from_secs(5)),
            )
            .await
            .map_err(|_| "Platform database is unavailable".to_owned())?;
        match command {
            super::ssh_cli::SshCommand::Help => println!("{}", super::ssh_cli::usage()),
            super::ssh_cli::SshCommand::List => {
                let rows = sqlx::query("SELECT id, fingerprint, purpose, requested_capabilities_json, granted_capabilities_json, status, version, requested_duration_seconds, granted_duration_seconds, expires_at_unix FROM platform_ssh_access_requests ORDER BY created_at_unix DESC LIMIT 200")
                    .fetch_all(&pool).await.map_err(|_| "could not list SSH requests".to_owned())?;
                if rows.is_empty() {
                    println!("No SSH access requests.");
                }
                for row in rows {
                    println!(
                        "{}  {}  v{}  {}  requested=[{}] {}s granted=[{}] {}s expires={}",
                        row.get::<String, _>("id"),
                        row.get::<String, _>("status"),
                        row.get::<i64, _>("version"),
                        row.get::<String, _>("fingerprint"),
                        row.get::<String, _>("requested_capabilities_json"),
                        row.get::<i64, _>("requested_duration_seconds"),
                        row.get::<String, _>("granted_capabilities_json"),
                        row.get::<Option<i64>, _>("granted_duration_seconds")
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "-".to_owned()),
                        row.get::<Option<i64>, _>("expires_at_unix")
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "-".to_owned())
                    );
                    println!("  {}", terminal_safe(&row.get::<String, _>("purpose")));
                }
            }
            super::ssh_cli::SshCommand::Show { request_id } => {
                let row = ssh_request_row(&pool, &request_id).await?;
                print_ssh_request(&row);
            }
            super::ssh_cli::SshCommand::Grant {
                request_id,
                capabilities,
                duration_seconds,
            } => {
                let row = ssh_request_row(&pool, &request_id).await?;
                print_ssh_request(&row);
                if row.get::<String, _>("status") != "pending" {
                    return Err("only a pending SSH request can be granted".to_owned());
                }
                let id = row.get::<String, _>("id");
                let version = row.get::<i64, _>("version");
                let created_at: i64 = row.get("created_at_unix");
                let request_cutoff = unix_now().saturating_sub(PENDING_TTL_SECONDS);
                if created_at <= request_cutoff {
                    if expire_pending_request(&pool, &id, version, request_cutoff).await? {
                        return Err("SSH request has expired; submit a new request".to_owned());
                    }
                    return Err("request changed while checking its expiry".to_owned());
                }
                let expires = unix_now().saturating_add(duration_seconds);
                let expires_display =
                    chrono_free_iso(expires).unwrap_or_else(|| format!("Unix time {expires}"));
                let username = ssh_username(&id);
                let host = ssh_access_host();
                let capabilities_display = capabilities.join(", ");
                println!(
                    "\nGrant preview: account {username}@{host}; scope [{capabilities_display}]; duration {duration_seconds} seconds; expires {expires_display}."
                );
                let fingerprint: String = row.get("fingerprint");
                println!("Type the displayed fingerprint exactly to create this temporary SSH account, key file, sudo policy, and expiry timer:");
                confirm_stdin(&fingerprint)?;
                let host_admin_confirmed = if capabilities
                    .iter()
                    .any(|capability| capability == "host_admin")
                {
                    eprintln!("WARNING: this grants unrestricted root access until expiry.");
                    println!("Type UNRESTRICTED ROOT to confirm:");
                    confirm_stdin("UNRESTRICTED ROOT")?;
                    true
                } else {
                    false
                };
                let granted_json = serde_json::to_string(&capabilities)
                    .map_err(|_| "could not encode grant scope".to_owned())?;
                let transition_cutoff = unix_now().saturating_sub(PENDING_TTL_SECONDS);
                let transition = sqlx::query("UPDATE platform_ssh_access_requests SET status='provisioning', version=version+1, granted_capabilities_json=?, granted_duration_seconds=?, expires_at_unix=?, decided_by_profile_id=NULL, decided_by_actor_kind='local_operator', decided_by_actor_label='local-root', decided_at_unix=?, operation_detail='granting' WHERE id=? AND status='pending' AND version=? AND created_at_unix > ?")
                    .bind(&granted_json).bind(duration_seconds).bind(expires).bind(unix_now()).bind(&id).bind(version).bind(transition_cutoff)
                    .execute(&pool).await.map_err(|_| "could not begin SSH grant".to_owned())?;
                if transition.rows_affected() != 1 {
                    if expire_pending_request(&pool, &id, version, transition_cutoff).await? {
                        return Err("SSH request expired before the grant could be recorded; submit a new request".to_owned());
                    }
                    return Err("request changed before grant; no access was created".to_owned());
                }
                let input = Input {
                    operation: Some("grant_ssh".to_owned()),
                    request_id: id.clone(),
                    expected_version: Some(version + 1),
                    session_token: None,
                    granted_capabilities: Some(capabilities),
                    granted_duration_seconds: Some(duration_seconds),
                    expires_at_unix: Some(expires),
                    expected_fingerprint: Some(fingerprint),
                    host_admin_confirmed: Some(host_admin_confirmed),
                };
                match grant_ssh(&pool, &id, &input).await {
                    Ok(()) => {
                        let result = sqlx::query("UPDATE platform_ssh_access_requests SET status='active', version=version+1, operation_detail=NULL WHERE id=? AND status='provisioning' AND version=?")
                            .bind(&id).bind(version + 1).execute(&pool).await.map_err(|_| "account created but grant state update failed; inspect with ssh show".to_owned())?;
                        if result.rows_affected() != 1 {
                            return Err("account created but grant state changed unexpectedly; inspect with ssh show".to_owned());
                        }
                        println!("Temporary SSH access active: {username}@{host}; scope [{capabilities_display}]; expires {expires_display}; request {id}.");
                    }
                    Err((_, detail)) => {
                        let _ = sqlx::query("UPDATE platform_ssh_access_requests SET status='failed', version=version+1, operation_detail=? WHERE id=? AND status='provisioning' AND version=?")
                            .bind(detail).bind(&id).bind(version + 1).execute(&pool).await;
                        return Err(format!("grant failed: {detail}; inspect with ssh show"));
                    }
                }
            }
            super::ssh_cli::SshCommand::Revoke { request_id } => {
                let row = ssh_request_row(&pool, &request_id).await?;
                print_ssh_request(&row);
                let status: String = row.get("status");
                if !matches!(status.as_str(), "active" | "failed") {
                    return Err("only active or failed grants can be revoked".to_owned());
                }
                println!("\nType YES to revoke this grant and remove its managed account, key, sudo policy, and expiry timer:");
                confirm_stdin("YES")?;
                let id: String = row.get("id");
                let version: i64 = row.get("version");
                let transition = sqlx::query("UPDATE platform_ssh_access_requests SET status='provisioning', version=version+1, decided_by_profile_id=NULL, decided_by_actor_kind='local_operator', decided_by_actor_label='local-root', decided_at_unix=?, operation_detail='revoking' WHERE id=? AND status IN ('active','failed') AND version=?")
                    .bind(unix_now()).bind(&id).bind(version).execute(&pool).await.map_err(|_| "could not begin SSH revocation".to_owned())?;
                if transition.rows_affected() != 1 {
                    return Err("grant changed before revocation".to_owned());
                }
                if let Err((_, detail)) = revoke_ssh(&pool, &id, version + 1).await {
                    let _ = sqlx::query("UPDATE platform_ssh_access_requests SET status='failed', version=version+1, operation_detail='local revocation failed; access may remain active' WHERE id=? AND status='provisioning' AND version=?")
                        .bind(&id).bind(version + 1).execute(&pool).await;
                    return Err(format!("revocation failed: {detail}"));
                }
                let result = sqlx::query("UPDATE platform_ssh_access_requests SET status='revoked', version=version+1, public_key='', operation_detail=NULL WHERE id=? AND status='provisioning' AND version=?")
                    .bind(&id).bind(version + 1).execute(&pool).await.map_err(|_| "account removed but revocation state update failed".to_owned())?;
                if result.rows_affected() != 1 {
                    return Err("account removed but grant state changed unexpectedly".to_owned());
                }
                println!("Temporary SSH grant {id} revoked.");
            }
            super::ssh_cli::SshCommand::Reconfigure { request_id } => {
                let row = ssh_request_row(&pool, &request_id).await?;
                print_ssh_request(&row);
                println!("\nGrant scope and duration are immutable after approval. Revoke this grant, then submit a new request for the desired scope.");
            }
        }
        Ok(())
    }

    async fn ssh_request_row(
        pool: &SqlitePool,
        id: &str,
    ) -> Result<sqlx::sqlite::SqliteRow, String> {
        sqlx::query("SELECT id, fingerprint, purpose, requested_capabilities_json, granted_capabilities_json, requested_duration_seconds, granted_duration_seconds, status, version, created_at_unix, expires_at_unix, operation_detail FROM platform_ssh_access_requests WHERE id=?")
            .bind(id).fetch_optional(pool).await.map_err(|_| "could not read SSH request".to_owned())?
            .ok_or_else(|| "SSH request does not exist".to_owned())
    }

    async fn expire_pending_request(
        pool: &SqlitePool,
        id: &str,
        version: i64,
        cutoff: i64,
    ) -> Result<bool, String> {
        let result = sqlx::query("UPDATE platform_ssh_access_requests SET status='expired', version=version+1, public_key='', decided_by_profile_id=NULL, decided_by_actor_kind='system_expiry', decided_by_actor_label='expiry-timer', operation_detail=NULL WHERE id=? AND status='pending' AND version=? AND created_at_unix <= ?")
            .bind(id)
            .bind(version)
            .bind(cutoff)
            .execute(pool)
            .await
            .map_err(|_| "could not expire stale SSH request".to_owned())?;
        Ok(result.rows_affected() == 1)
    }

    fn print_ssh_request(row: &sqlx::sqlite::SqliteRow) {
        println!("Request: {}", row.get::<String, _>("id"));
        println!(
            "Status: {} (version {})",
            row.get::<String, _>("status"),
            row.get::<i64, _>("version")
        );
        println!("Key fingerprint: {}", row.get::<String, _>("fingerprint"));
        println!(
            "Purpose: {}",
            terminal_safe(&row.get::<String, _>("purpose"))
        );
        println!(
            "Requested capabilities: {}",
            row.get::<String, _>("requested_capabilities_json")
        );
        println!(
            "Requested duration: {} seconds",
            row.get::<i64, _>("requested_duration_seconds")
        );
        println!(
            "Granted capabilities: {}",
            row.get::<String, _>("granted_capabilities_json")
        );
        println!(
            "Granted duration: {}",
            row.get::<Option<i64>, _>("granted_duration_seconds")
                .map(|v| format!("{v} seconds"))
                .unwrap_or_else(|| "-".to_owned())
        );
        println!(
            "Expires: {}",
            row.get::<Option<i64>, _>("expires_at_unix")
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".to_owned())
        );
        if let Some(detail) = row.get::<Option<String>, _>("operation_detail") {
            println!("Detail: {}", terminal_safe(&detail));
        }
    }

    fn terminal_safe(value: &str) -> String {
        value
            .chars()
            .map(|character| {
                if character.is_control() {
                    ' '
                } else {
                    character
                }
            })
            .collect()
    }

    fn confirm_stdin(expected: &str) -> Result<(), String> {
        use std::io::{self, IsTerminal, Write};
        if !io::stdin().is_terminal() {
            return Err("interactive terminal required; no change made".to_owned());
        }
        print!("> ");
        io::stdout()
            .flush()
            .map_err(|_| "could not prompt for confirmation".to_owned())?;
        let mut answer = String::new();
        io::stdin()
            .read_line(&mut answer)
            .map_err(|_| "could not read confirmation".to_owned())?;
        if answer.trim() == expected {
            Ok(())
        } else {
            Err("confirmation did not match; no change made".to_owned())
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
        if !super::valid_identity_authz_endpoint(&endpoint) {
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
        let Ok(mut response) = client
            .post(endpoint)
            .bearer_auth(service_token)
            .json(&request)
            .send()
            .await
        else {
            return false;
        };
        if response.status() != reqwest::StatusCode::OK
            || response.content_length().is_some_and(|length| {
                length > super::MAX_IDENTITY_AUTHORIZATION_RESPONSE_BYTES as u64
            })
        {
            return false;
        }
        let mut body = Vec::new();
        loop {
            match response.chunk().await {
                Ok(Some(chunk)) => {
                    if body.len().saturating_add(chunk.len())
                        > super::MAX_IDENTITY_AUTHORIZATION_RESPONSE_BYTES
                    {
                        return false;
                    }
                    body.extend_from_slice(&chunk);
                }
                Ok(None) => break,
                Err(_) => return false,
            }
        }
        super::identity_response_allows_root(reqwest::StatusCode::OK, &body, unix_now())
    }

    async fn verify_deploy_request(
        pool: &SqlitePool,
        id: &str,
        version: i64,
    ) -> Result<(), (String, &'static str)> {
        if version < 1 {
            return Err((id.to_string(), "invalid approved deploy version"));
        }
        let row = sqlx::query(
            "SELECT status, version, request_json FROM platform_deploy_requests WHERE id = ?",
        )
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
        let request_json: String = row.get("request_json");
        let request: serde_json::Value = serde_json::from_str(&request_json)
            .map_err(|_| (id.to_string(), "approved deploy request is invalid"))?;
        if !super::valid_deployment_target(
            request
                .get("server_mode")
                .and_then(serde_json::Value::as_str),
        ) {
            return Err((
                id.to_string(),
                "approved deploy request has no valid target",
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

        record_stopped_deploy_failure(pool, id, version).await
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
        create_managed_account(pool, id, version, &public_key, granted, expires).await?;
        Ok(())
    }

    async fn create_managed_account(
        pool: &SqlitePool,
        id: &str,
        expected_version: i64,
        public_key: &str,
        capabilities: &[String],
        expires: i64,
    ) -> Result<(), (String, &'static str)> {
        let _lifecycle_lock = acquire_cleanup_lock(id)?;
        let row = sqlx::query(
            "SELECT status, version, operation_detail, expires_at_unix
             FROM platform_ssh_access_requests WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|_| (id.to_string(), "could not recheck SSH grant state"))?;
        let Some(row) = row else {
            return Err((id.to_string(), "SSH grant no longer exists"));
        };
        if row.get::<String, _>("status") != "provisioning"
            || row.get::<i64, _>("version") != expected_version
            || row.get::<Option<String>, _>("operation_detail").as_deref() != Some("granting")
            || row.get::<Option<i64>, _>("expires_at_unix") != Some(expires)
            || expires <= unix_now()
        {
            return Err((id.to_string(), "SSH grant is no longer current"));
        }
        let username = ssh_username(id);
        if account_exists(&username) {
            return Err((
                id.to_string(),
                "temporary account already exists; refusing duplicate grant",
            ));
        }
        match create_managed_account_locked(id, public_key, capabilities, expires).await {
            Ok(()) => Ok(()),
            Err((_, primary_failure)) => {
                let rollback_completed = remove_managed_account_locked(id).await.is_ok();
                Err((
                    id.to_string(),
                    super::grant_setup_failure_detail(primary_failure, rollback_completed),
                ))
            }
        }
    }

    async fn create_managed_account_locked(
        id: &str,
        public_key: &str,
        capabilities: &[String],
        expires: i64,
    ) -> Result<(), (String, &'static str)> {
        let username = ssh_username(id);
        let hash = random_password_hash(id).await?;
        if run_command(
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
        .is_err()
        {
            return Err((id.to_string(), "could not create temporary account"));
        }
        let home = format!("/home/{username}");
        let ssh_dir = format!("{home}/.ssh");
        if run_command("/usr/bin/chown", &["root:root", &home])
            .await
            .is_err()
            || run_command("/usr/bin/chmod", &["0755", &home])
                .await
                .is_err()
        {
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
            return Err((id.to_string(), "could not prepare temporary workspace"));
        }
        if run_command(
            "/usr/bin/install",
            &["-d", "-o", "root", "-g", "root", "-m", "0755", &ssh_dir],
        )
        .await
        .is_err()
        {
            return Err((
                id.to_string(),
                "could not prepare managed SSH key directory",
            ));
        }
        if install_expiry_units(id, expires).await.is_err() {
            return Err((id.to_string(), "could not install persistent expiry timer"));
        }
        let key_path = format!("{ssh_dir}/authorized_keys");
        let key_line = format!("{public_key}\n");
        if atomic_write_root_file(&key_path, key_line.as_bytes(), 0o644).is_err() {
            return Err((id.to_string(), "could not install managed public key"));
        }
        let sudoers_path = format!("/etc/sudoers.d/vapor-ssh-{id}");
        let sudoers = sudoers_contents(&username, capabilities);
        if atomic_write_root_file(&sudoers_path, sudoers.as_bytes(), 0o440).is_err()
            || run_command("/usr/sbin/visudo", &["-cf", &sudoers_path])
                .await
                .is_err()
        {
            return Err((id.to_string(), "scoped sudo policy did not validate"));
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
        remove_managed_account_locked(id).await
    }

    async fn remove_managed_account_locked(id: &str) -> Result<(), (String, &'static str)> {
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

    fn ssh_access_host() -> String {
        configured_value("VAPOR_SSH_ACCESS_HOST")
            .or_else(|| configured_value("VAPOR_DOMAIN"))
            .unwrap_or_else(|| "vapor.ghf-studios.site".to_owned())
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
        let _cleanup_lock = acquire_cleanup_lock(id).map_err(|_| "could not lock SSH grant")?;
        let row = sqlx::query(
            "SELECT status, version, expires_at_unix FROM platform_ssh_access_requests WHERE id=?",
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
        let version: i64 = row.get("version");
        let expires: i64 = row.get("expires_at_unix");
        let now = unix_now();
        let claimed = sqlx::query(
            "UPDATE platform_ssh_access_requests
             SET status='provisioning', version=version+1, decided_by_profile_id=NULL,
                 decided_by_actor_kind='system_expiry', decided_by_actor_label='expiry-timer',
                 decided_at_unix=?, operation_detail='expiring'
             WHERE id=? AND status IN ('active','provisioning','failed') AND version=?
               AND expires_at_unix=? AND expires_at_unix<=?",
        )
        .bind(now)
        .bind(id)
        .bind(version)
        .bind(expires)
        .bind(now)
        .execute(&pool)
        .await
        .map_err(|_| "could not claim grant expiry")?;
        if claimed.rows_affected() != 1 {
            return Err("SSH grant changed before expiry cleanup");
        }
        let expiring_version = version + 1;
        if remove_managed_account_locked(id).await.is_err() {
            let _ = sqlx::query(
                "UPDATE platform_ssh_access_requests
                 SET status='failed', version=version+1,
                     operation_detail='expiry cleanup failed; access may remain active'
                 WHERE id=? AND status='provisioning' AND version=? AND operation_detail='expiring'",
            )
            .bind(id)
            .bind(expiring_version)
            .execute(&pool)
            .await;
            return Err("temporary account cleanup failed; access may remain active");
        }
        let completed = sqlx::query(
            "UPDATE platform_ssh_access_requests
             SET status='expired', version=version+1, public_key='',
                 decided_by_profile_id=NULL, decided_by_actor_kind='system_expiry',
                 decided_by_actor_label='expiry-timer', operation_detail=NULL
             WHERE id=? AND status='provisioning' AND version=? AND operation_detail='expiring'",
        )
        .bind(id)
        .bind(expiring_version)
        .execute(&pool)
        .await
        .map_err(|_| "could not record grant expiry")?;
        if completed.rows_affected() != 1 {
            return Err("grant cleanup completed but expiry state needs reconciliation");
        }
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
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.first().is_some_and(|value| value == "ssh") {
        match ssh_cli::parse_ssh_args(&args[1..]) {
            Ok(ssh_cli::SshCommand::Help) => println!("{}", ssh_cli::usage()),
            Ok(_) => {
                eprintln!("error: typed SSH operations require a Linux VPS");
                std::process::exit(2);
            }
            Err(error) => {
                eprintln!("error: {error}\n{}", ssh_cli::usage());
                std::process::exit(2);
            }
        }
        return;
    }
    eprintln!("vapor_platform_ops is a Linux server helper and cannot run on this platform");
}

#[cfg(test)]
mod endpoint_tests {
    use super::{
        record_stopped_deploy_failure, valid_deployment_target, valid_identity_authz_endpoint,
    };
    use sqlx::sqlite::SqlitePoolOptions;
    use sqlx::Row;

    #[tokio::test]
    async fn reconciliation_closes_the_dispatch_lifecycle() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE platform_deploy_requests (
                id TEXT PRIMARY KEY,
                status TEXT NOT NULL,
                version INTEGER NOT NULL,
                dispatch_state TEXT NOT NULL,
                dispatch_detail TEXT,
                result_detail TEXT
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO platform_deploy_requests
             (id, status, version, dispatch_state) VALUES ('request-1', 'executing', 4, 'claimed')",
        )
        .execute(&pool)
        .await
        .unwrap();

        record_stopped_deploy_failure(&pool, "request-1", 4)
            .await
            .unwrap();

        let row = sqlx::query(
            "SELECT status, dispatch_state FROM platform_deploy_requests WHERE id='request-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.get::<String, _>("status"), "failed");
        assert_eq!(row.get::<String, _>("dispatch_state"), "unknown");
    }

    #[test]
    fn deployment_target_must_be_explicit_and_supported() {
        assert!(valid_deployment_target(Some("monolith")));
        assert!(valid_deployment_target(Some("services")));
        assert!(!valid_deployment_target(None));
        assert!(!valid_deployment_target(Some("automatic")));
        assert!(!valid_deployment_target(Some("Services")));
    }

    #[test]
    fn accepts_the_two_configured_identity_topologies() {
        assert!(valid_identity_authz_endpoint(
            "http://127.0.0.1:7110/api/identity/v1/internal/authorization/check"
        ));
        assert!(valid_identity_authz_endpoint(
            "http://127.0.0.1:7113/v1/internal/authorization/check"
        ));
    }

    #[test]
    fn rejects_other_endpoints_and_url_variants() {
        for endpoint in [
            "http://localhost:7110/api/identity/v1/internal/authorization/check",
            "http://127.0.0.1:7111/api/identity/v1/internal/authorization/check",
            "https://127.0.0.1:7110/api/identity/v1/internal/authorization/check",
            "http://user@127.0.0.1:7110/api/identity/v1/internal/authorization/check",
            "http://127.0.0.1:7113/v1/internal/authorization/check?x=1",
            "http://127.0.0.1:7113/v1/internal/authorization/check/",
        ] {
            assert!(!valid_identity_authz_endpoint(endpoint), "{endpoint}");
        }
    }

    #[test]
    fn privileged_helper_rejects_expired_or_denied_identity_decisions() {
        assert!(!super::identity_decision_is_current(true, 99, 100));
        assert!(super::identity_decision_is_current(true, 101, 100));
        assert!(!super::identity_decision_is_current(false, 101, 100));
    }

    #[test]
    fn privileged_helper_requires_bounded_exact_success_identity_response() {
        let body = br#"{"allowed":true,"expires_at_unix":101}"#;
        assert!(super::identity_response_allows_root(
            reqwest::StatusCode::OK,
            body,
            100
        ));
        assert!(!super::identity_response_allows_root(
            reqwest::StatusCode::CREATED,
            body,
            100
        ));
        assert!(!super::identity_response_allows_root(
            reqwest::StatusCode::OK,
            &vec![b' '; super::MAX_IDENTITY_AUTHORIZATION_RESPONSE_BYTES + 1],
            100
        ));
        assert!(!super::identity_response_allows_root(
            reqwest::StatusCode::OK,
            br#"{"allowed":true}"#,
            100
        ));
    }

    #[test]
    fn incomplete_grant_rollback_is_reported_as_recoverable_host_state() {
        assert_eq!(
            super::grant_setup_failure_detail("could not create temporary account", true),
            "could not create temporary account"
        );
        assert!(
            super::grant_setup_failure_detail("ignored", false).contains("rollback was incomplete")
        );
        assert!(super::grant_setup_failure_detail("ignored", false).contains("retry revoke"));
    }
}
