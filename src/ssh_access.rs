use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::header::CACHE_CONTROL;
use axum::http::{HeaderMap, HeaderValue, Request, StatusCode};
use axum::middleware::{from_fn, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::path::PathBuf;
use std::time::Duration;

const MAX_DURATION_SECONDS: i64 = 8 * 60 * 60;
const MAX_REQUESTS: i64 = 200;
const MAX_OPEN_REQUESTS_PER_PROFILE: i64 = 3;
const PENDING_TTL_SECONDS: i64 = 3 * 24 * 60 * 60;
const MAX_PURPOSE_BYTES: usize = 500;
const CAPABILITIES: &[&str] = &["vapor_deploy", "vapor_service_control", "host_admin"];

#[derive(Clone)]
struct SshAccessState {
    pool: SqlitePool,
    client: reqwest::Client,
    identity_root_authz_url: String,
    identity_root_authz_token: Option<String>,
    public_origin: String,
    ssh_host: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateSshRequest {
    public_key: String,
    purpose: String,
    requested_capabilities: Vec<String>,
    requested_duration_seconds: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApproveSshRequest {
    expected_version: i64,
    confirmed: bool,
    host_admin_confirmed: bool,
    granted_capabilities: Vec<String>,
    granted_duration_seconds: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RevokeSshRequest {
    expected_version: i64,
    confirmed: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RejectSshRequest {
    expected_version: i64,
    confirmed: bool,
}

#[derive(Serialize)]
struct SshRequestSummary {
    id: String,
    host: String,
    username: Option<String>,
    fingerprint: String,
    purpose: String,
    requested_capabilities: Vec<String>,
    granted_capabilities: Vec<String>,
    requested_duration_seconds: i64,
    granted_duration_seconds: Option<i64>,
    status: String,
    detail: Option<String>,
    version: i64,
    created_at_unix: i64,
    expires_at_unix: Option<i64>,
}

#[derive(Serialize)]
struct CreatedSshRequest {
    id: String,
    fingerprint: String,
    status: &'static str,
    version: i64,
}

#[derive(Serialize)]
struct RootOperation<'a> {
    operation: &'a str,
    request_id: &'a str,
    expected_version: i64,
    session_token: &'a str,
    expected_fingerprint: Option<&'a str>,
    host_admin_confirmed: bool,
    granted_capabilities: &'a [String],
    granted_duration_seconds: i64,
    expires_at_unix: Option<i64>,
}

pub(crate) fn router() -> Router {
    Router::new()
        .route("/api/ssh-access/requests", post(create_request))
        .route("/api/admin/ssh-access/requests", get(list_requests))
        .route(
            "/api/admin/ssh-access/requests/{id}/approve",
            post(approve_request),
        )
        .route(
            "/api/admin/ssh-access/requests/{id}/reject",
            post(reject_request),
        )
        .route(
            "/api/admin/ssh-access/grants/{id}/revoke",
            post(revoke_grant),
        )
        .with_state(state())
        .route_layer(from_fn(no_store))
}

pub(crate) async fn initialize() -> Result<(), Box<dyn std::error::Error>> {
    let directory = state_directory();
    tokio::fs::create_dir_all(directory).await?;
    let pool = create_pool();
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS platform_ssh_access_requests (
            id TEXT PRIMARY KEY,
            public_key TEXT NOT NULL,
            fingerprint TEXT NOT NULL,
            purpose TEXT NOT NULL,
            requested_capabilities_json TEXT NOT NULL,
            granted_capabilities_json TEXT NOT NULL DEFAULT '[]',
            requested_duration_seconds INTEGER NOT NULL CHECK(requested_duration_seconds BETWEEN 1 AND 28800),
            granted_duration_seconds INTEGER,
            requester_credential_id TEXT NOT NULL,
            requester_profile_id TEXT NOT NULL,
            status TEXT NOT NULL CHECK(status IN ('pending','provisioning','active','failed','rejected','revoked','expired')),
            version INTEGER NOT NULL DEFAULT 1 CHECK(version > 0),
            created_at_unix INTEGER NOT NULL,
            expires_at_unix INTEGER,
            decided_by_profile_id TEXT,
            decided_at_unix INTEGER,
            operation_detail TEXT
        )",
    )
    .execute(&pool)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_platform_ssh_access_status ON platform_ssh_access_requests(status, created_at_unix)")
        .execute(&pool)
        .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS platform_ssh_access_events (
            event_id INTEGER PRIMARY KEY AUTOINCREMENT,
            request_id TEXT NOT NULL,
            from_status TEXT,
            to_status TEXT NOT NULL,
            actor_profile_id TEXT,
            at_unix INTEGER NOT NULL
        )",
    )
    .execute(&pool)
    .await?;
    sqlx::query(
        "CREATE TRIGGER IF NOT EXISTS platform_ssh_access_insert_audit
         AFTER INSERT ON platform_ssh_access_requests
         BEGIN
           INSERT INTO platform_ssh_access_events(request_id, from_status, to_status, actor_profile_id, at_unix)
           VALUES (NEW.id, NULL, NEW.status, NEW.requester_profile_id, NEW.created_at_unix);
         END",
    )
    .execute(&pool)
    .await?;
    sqlx::query(
        "CREATE TRIGGER IF NOT EXISTS platform_ssh_access_update_audit
         AFTER UPDATE OF status ON platform_ssh_access_requests
         WHEN OLD.status != NEW.status
         BEGIN
           INSERT INTO platform_ssh_access_events(request_id, from_status, to_status, actor_profile_id, at_unix)
           VALUES (NEW.id, OLD.status, NEW.status, NEW.decided_by_profile_id, CAST(strftime('%s','now') AS INTEGER));
         END",
    )
    .execute(&pool)
    .await?;
    Ok(())
}

fn state_directory() -> PathBuf {
    std::env::var_os("VAPOR_PLATFORM_STATE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/var/lib/vapor-server/platform"))
}

fn create_pool() -> SqlitePool {
    let database_path = std::env::var_os("VAPOR_PLATFORM_DB")
        .map(PathBuf::from)
        .unwrap_or_else(|| state_directory().join("platform.sqlite3"));
    let options = SqliteConnectOptions::new()
        .filename(database_path)
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);
    SqlitePoolOptions::new()
        .max_connections(5)
        .connect_lazy_with(options)
}

fn state() -> SshAccessState {
    let domain =
        std::env::var("VAPOR_DOMAIN").unwrap_or_else(|_| "vapor.ghf-studios.site".to_string());
    SshAccessState {
        pool: create_pool(),
        client: reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("valid Platform HTTP client configuration"),
        identity_root_authz_url: std::env::var("VAPOR_PLATFORM_IDENTITY_AUTHZ_URL").unwrap_or_else(
            |_| "http://127.0.0.1:7110/api/identity/v1/internal/authorization/check".to_string(),
        ),
        identity_root_authz_token: std::env::var("VAPOR_PLATFORM_IDENTITY_AUTHZ_TOKEN").ok(),
        public_origin: std::env::var("VAPOR_PLATFORM_PUBLIC_ORIGIN")
            .unwrap_or_else(|_| format!("https://{domain}")),
        ssh_host: std::env::var("VAPOR_SSH_ACCESS_HOST").unwrap_or(domain),
    }
}

async fn create_request(
    State(state): State<SshAccessState>,
    headers: HeaderMap,
    Json(request): Json<CreateSshRequest>,
) -> Response {
    let Some(token) = bearer_token(&headers) else {
        return (StatusCode::UNAUTHORIZED, "request credential required\n").into_response();
    };
    if let Err(message) = validate_request(&request) {
        return (StatusCode::BAD_REQUEST, message).into_response();
    }
    let key = match crate::ssh_key::parse_ed25519_public_key(&request.public_key) {
        Ok(key) => key,
        Err(()) => {
            return (StatusCode::BAD_REQUEST, "invalid ssh-ed25519 public key\n").into_response()
        }
    };
    let credential = match crate::deploy_requests::verify_platform_request_credential(
        token,
        "platform:ssh-access:request",
    )
    .await
    {
        Ok(credential) => credential,
        Err(status) => return status.into_response(),
    };
    let request_hash = ssh_request_binding_hash(&key.fingerprint, &request);
    if credential.bound_ssh_fingerprint.as_deref() != Some(key.fingerprint.as_str())
        || credential.bound_ssh_request_hash.as_deref() != Some(request_hash.as_str())
    {
        return (
            StatusCode::FORBIDDEN,
            "authorization is bound to a different SSH key or request\n",
        )
            .into_response();
    }
    if expire_stale_requests(&state.pool).await.is_err() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let open_count: i64 = match sqlx::query_scalar(
        "SELECT COUNT(*) FROM platform_ssh_access_requests
         WHERE (requester_profile_id = ? OR fingerprint = ?)
         AND status IN ('pending','provisioning','active','failed')",
    )
    .bind(&credential.actor_profile_id)
    .bind(&key.fingerprint)
    .fetch_one(&state.pool)
    .await
    {
        Ok(value) => value,
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    if open_count >= MAX_OPEN_REQUESTS_PER_PROFILE {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            "too many open requests for this profile or key\n",
        )
            .into_response();
    }
    let requested = serde_json::to_string(&request.requested_capabilities).unwrap_or_default();
    let id = uuid::Uuid::new_v4().to_string();
    let result = sqlx::query(
        "INSERT INTO platform_ssh_access_requests
         (id, public_key, fingerprint, purpose, requested_capabilities_json, requester_credential_id,
          requester_profile_id, requested_duration_seconds, status, version, created_at_unix)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'pending', 1, ?)",
    )
    .bind(&id)
    .bind(key.canonical)
    .bind(&key.fingerprint)
    .bind(request.purpose.trim())
    .bind(requested)
    .bind(credential.credential_id)
    .bind(credential.actor_profile_id)
    .bind(request.requested_duration_seconds)
    .bind(unix_now())
    .execute(&state.pool)
    .await;
    if result.is_err() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    (
        StatusCode::ACCEPTED,
        Json(CreatedSshRequest {
            id,
            fingerprint: key.fingerprint,
            status: "pending",
            version: 1,
        }),
    )
        .into_response()
}

async fn list_requests(State(state): State<SshAccessState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_root(&state, &headers).await {
        return response;
    }
    if expire_stale_requests(&state.pool).await.is_err() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let rows = match sqlx::query(
        "SELECT id, fingerprint, purpose, requested_capabilities_json, granted_capabilities_json,
         requested_duration_seconds, granted_duration_seconds, status, version, created_at_unix, expires_at_unix, operation_detail
         FROM platform_ssh_access_requests ORDER BY created_at_unix DESC LIMIT ?",
    )
    .bind(MAX_REQUESTS)
    .fetch_all(&state.pool)
    .await
    {
        Ok(rows) => rows,
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let requests = rows
        .into_iter()
        .filter_map(|row| {
            Some(SshRequestSummary {
                id: row.try_get("id").ok()?,
                host: state.ssh_host.clone(),
                username: (row.try_get::<&str, _>("status").ok()? == "active").then(|| {
                    format!(
                        "vpssh_{}",
                        row.try_get::<&str, _>("id")
                            .unwrap_or("")
                            .split('-')
                            .next()
                            .unwrap_or("")
                    )
                }),
                fingerprint: row.try_get("fingerprint").ok()?,
                purpose: row.try_get("purpose").ok()?,
                requested_capabilities: serde_json::from_str(
                    row.try_get::<&str, _>("requested_capabilities_json").ok()?,
                )
                .ok()?,
                granted_capabilities: serde_json::from_str(
                    row.try_get::<&str, _>("granted_capabilities_json").ok()?,
                )
                .ok()?,
                requested_duration_seconds: row.try_get("requested_duration_seconds").ok()?,
                granted_duration_seconds: row.try_get("granted_duration_seconds").ok()?,
                status: row.try_get("status").ok()?,
                detail: row.try_get("operation_detail").ok()?,
                version: row.try_get("version").ok()?,
                created_at_unix: row.try_get("created_at_unix").ok()?,
                expires_at_unix: row.try_get("expires_at_unix").ok()?,
            })
        })
        .collect::<Vec<_>>();
    Json(requests).into_response()
}

async fn approve_request(
    State(state): State<SshAccessState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<ApproveSshRequest>,
) -> Response {
    if !valid_id(&id)
        || request.expected_version < 1
        || !request.confirmed
        || request.granted_duration_seconds < 1
        || request.granted_duration_seconds > MAX_DURATION_SECONDS
        || validate_capabilities(&request.granted_capabilities).is_err()
    {
        return (StatusCode::BAD_REQUEST, "invalid grant decision\n").into_response();
    }
    if !same_origin(&state, &headers) {
        return (StatusCode::FORBIDDEN, "same-origin request required\n").into_response();
    }
    let actor = match require_root(&state, &headers).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let Some(session_token) = crate::admin_auth::session_token(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let row = match sqlx::query(
        "SELECT fingerprint FROM platform_ssh_access_requests
         WHERE id = ? AND status = 'pending' AND version = ?",
    )
    .bind(&id)
    .bind(request.expected_version)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(Some(row)) => row,
        Ok(None) => return StatusCode::CONFLICT.into_response(),
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let fingerprint: String = row.try_get("fingerprint").unwrap_or_default();
    if (request
        .granted_capabilities
        .iter()
        .any(|value| value == "host_admin")
        && request.granted_capabilities.len() != 1)
        || (request
            .granted_capabilities
            .iter()
            .any(|value| value == "host_admin")
            && !request.host_admin_confirmed)
    {
        return (
            StatusCode::BAD_REQUEST,
            "host-admin must be granted alone and requires explicit confirmation\n",
        )
            .into_response();
    }
    let expires = unix_now().saturating_add(request.granted_duration_seconds);
    let transitioning = sqlx::query(
        "UPDATE platform_ssh_access_requests SET status = 'provisioning', version = version + 1,
         decided_by_profile_id = ?, decided_at_unix = ?, expires_at_unix = ?, operation_detail='granting'
         WHERE id = ? AND status = 'pending' AND version = ?",
    )
    .bind(actor)
    .bind(unix_now())
    .bind(expires)
    .bind(&id)
    .bind(request.expected_version)
    .execute(&state.pool)
    .await;
    if !matches!(transitioning, Ok(result) if result.rows_affected() == 1) {
        return StatusCode::CONFLICT.into_response();
    }
    let payload = RootOperation {
        operation: "grant_ssh",
        request_id: &id,
        expected_version: request.expected_version + 1,
        session_token,
        expected_fingerprint: Some(&fingerprint),
        host_admin_confirmed: request.host_admin_confirmed,
        granted_capabilities: &request.granted_capabilities,
        granted_duration_seconds: request.granted_duration_seconds,
        expires_at_unix: Some(expires),
    };
    if run_root_operation(&payload).await.is_err() {
        let _ = sqlx::query(
            "UPDATE platform_ssh_access_requests SET status = 'failed', version = version + 1,
             operation_detail = 'privileged grant helper failed' WHERE id = ? AND status = 'provisioning'",
        )
        .bind(&id)
        .execute(&state.pool)
        .await;
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "grant helper failed; no access was confirmed\n",
        )
            .into_response();
    }
    let granted = serde_json::to_string(&request.granted_capabilities).unwrap_or_default();
    let active_result = sqlx::query(
        "UPDATE platform_ssh_access_requests SET status = 'active', version = version + 1,
         granted_capabilities_json = ?, granted_duration_seconds = ?, expires_at_unix = ?,
         operation_detail = NULL WHERE id = ? AND status = 'provisioning' AND version = ?",
    )
    .bind(granted)
    .bind(request.granted_duration_seconds)
    .bind(expires)
    .bind(&id)
    .bind(request.expected_version + 1)
    .execute(&state.pool)
    .await;
    if !matches!(active_result, Ok(result) if result.rows_affected() == 1) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    (
        StatusCode::OK,
        Json(serde_json::json!({"id":id,"status":"active","expires_at_unix":expires})),
    )
        .into_response()
}

async fn reject_request(
    State(state): State<SshAccessState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<RejectSshRequest>,
) -> Response {
    if !valid_id(&id) || request.expected_version < 1 || !request.confirmed {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if !same_origin(&state, &headers) {
        return (StatusCode::FORBIDDEN, "same-origin request required\n").into_response();
    }
    let actor = match require_root(&state, &headers).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let result = sqlx::query(
        "UPDATE platform_ssh_access_requests SET status='rejected', version=version+1,
         public_key='', decided_by_profile_id=?, decided_at_unix=?, operation_detail='request rejected'
         WHERE id=? AND status='pending' AND version=?",
    )
    .bind(actor)
    .bind(unix_now())
    .bind(&id)
    .bind(request.expected_version)
    .execute(&state.pool)
    .await;
    if !matches!(result, Ok(value) if value.rows_affected() == 1) {
        return StatusCode::CONFLICT.into_response();
    }
    (
        StatusCode::OK,
        Json(serde_json::json!({"id":id,"status":"rejected"})),
    )
        .into_response()
}

async fn revoke_grant(
    State(state): State<SshAccessState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<RevokeSshRequest>,
) -> Response {
    if !valid_id(&id) || request.expected_version < 1 || !request.confirmed {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if !same_origin(&state, &headers) {
        return (StatusCode::FORBIDDEN, "same-origin request required\n").into_response();
    }
    let actor = match require_root(&state, &headers).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let Some(session_token) = crate::admin_auth::session_token(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let transition = sqlx::query(
        "UPDATE platform_ssh_access_requests SET status = 'provisioning', version = version + 1,
         decided_by_profile_id = ?, decided_at_unix = ?, operation_detail='revoking'
         WHERE id = ? AND status IN ('active','failed') AND version = ?",
    )
    .bind(actor)
    .bind(unix_now())
    .bind(&id)
    .bind(request.expected_version)
    .execute(&state.pool)
    .await;
    if !matches!(transition, Ok(result) if result.rows_affected() == 1) {
        return StatusCode::CONFLICT.into_response();
    }
    let payload = RootOperation {
        operation: "revoke_ssh",
        request_id: &id,
        expected_version: request.expected_version + 1,
        session_token,
        expected_fingerprint: None,
        host_admin_confirmed: false,
        granted_capabilities: &[],
        granted_duration_seconds: 0,
        expires_at_unix: None,
    };
    if run_root_operation(&payload).await.is_err() {
        let _ = sqlx::query(
            "UPDATE platform_ssh_access_requests SET status = 'failed', version = version + 1,
             operation_detail = 'revocation helper failed; access may remain active' WHERE id = ? AND status = 'provisioning'",
        )
        .bind(&id)
        .execute(&state.pool)
        .await;
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "revocation failed; access may remain active\n",
        )
            .into_response();
    }
    let result = sqlx::query(
        "UPDATE platform_ssh_access_requests SET status = 'revoked', version = version + 1,
         public_key = '', operation_detail = NULL WHERE id = ? AND status = 'provisioning' AND version = ?",
    )
    .bind(&id)
    .bind(request.expected_version + 1)
    .execute(&state.pool)
    .await;
    if !matches!(result, Ok(row) if row.rows_affected() == 1) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    (
        StatusCode::OK,
        Json(serde_json::json!({"id":id,"status":"revoked"})),
    )
        .into_response()
}

async fn run_root_operation(payload: &RootOperation<'_>) -> Result<(), ()> {
    let operation_path = std::env::var_os("VAPOR_PLATFORM_OPS_PATH")
        .unwrap_or_else(|| "/usr/local/libexec/vapor-platform-ops".into());
    let mut child = tokio::process::Command::new("/usr/bin/sudo")
        .arg("-n")
        .arg(operation_path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| ())?;
    let input = serde_json::to_vec(payload).map_err(|_| ())?;
    if input.len() > 4096 {
        return Err(());
    }
    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        stdin.write_all(&input).await.map_err(|_| ())?;
    } else {
        return Err(());
    }
    let output = tokio::time::timeout(Duration::from_secs(10), child.wait_with_output())
        .await
        .map_err(|_| ())?
        .map_err(|_| ())?;
    if !output.status.success() || output.stdout.len() > 4096 {
        return Err(());
    }
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).map_err(|_| ())?;
    if result.get("ok").and_then(serde_json::Value::as_bool) == Some(true) {
        Ok(())
    } else {
        Err(())
    }
}

async fn require_root(state: &SshAccessState, headers: &HeaderMap) -> Result<String, Response> {
    crate::admin_auth::authorize_root_profile_with_client(
        headers,
        state.identity_root_authz_token.as_deref(),
        &state.identity_root_authz_url,
        &state.client,
    )
    .await
    .map_err(|failure| {
        let mut response = (failure.status(), "root dashboard session required\n").into_response();
        response
            .headers_mut()
            .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
        response
    })
}

fn validate_request(request: &CreateSshRequest) -> Result<(), &'static str> {
    if request.purpose.trim().is_empty()
        || request.purpose.len() > MAX_PURPOSE_BYTES
        || request.purpose.chars().any(char::is_control)
    {
        return Err("purpose must be plain text of 1–500 bytes\n");
    }
    if !(1..=MAX_DURATION_SECONDS).contains(&request.requested_duration_seconds) {
        return Err("requested duration must be from 1 second to 8 hours\n");
    }
    validate_capabilities(&request.requested_capabilities)
}

fn validate_capabilities(capabilities: &[String]) -> Result<(), &'static str> {
    if capabilities.len() > CAPABILITIES.len()
        || capabilities
            .iter()
            .any(|item| !CAPABILITIES.contains(&item.as_str()))
        || capabilities
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != capabilities.len()
        || (capabilities.iter().any(|item| item == "host_admin") && capabilities.len() != 1)
    {
        return Err("unsupported, duplicate, or conflicting capability\n");
    }
    Ok(())
}

fn ssh_request_binding_hash(fingerprint: &str, request: &CreateSshRequest) -> String {
    let mut capabilities = request.requested_capabilities.clone();
    capabilities.sort_unstable();
    let canonical = format!(
        "{}\n{}\n{}\n{}",
        fingerprint,
        request.purpose.trim(),
        capabilities.join(" "),
        request.requested_duration_seconds
    );
    crate::deploy_requests::sha256_hex(canonical.as_bytes())
}

fn valid_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok()
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}

fn same_origin(state: &SshAccessState, headers: &HeaderMap) -> bool {
    headers.get("origin").and_then(|value| value.to_str().ok())
        == Some(state.public_origin.as_str())
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

async fn expire_stale_requests(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let now = unix_now();
    let cutoff = now.saturating_sub(PENDING_TTL_SECONDS);
    let interrupted_cutoff = now.saturating_sub(120);
    sqlx::query(
        "UPDATE platform_ssh_access_requests SET status='expired', version=version+1,
         public_key='', decided_by_profile_id=NULL, operation_detail=NULL
         WHERE status='pending' AND created_at_unix <= ?",
    )
    .bind(cutoff)
    .execute(pool)
    .await?;
    sqlx::query(
        "UPDATE platform_ssh_access_requests SET status='failed', version=version+1,
         operation_detail='interrupted privileged operation; cleanup may be required'
         WHERE status='provisioning' AND decided_at_unix <= ?",
    )
    .bind(interrupted_cutoff)
    .execute(pool)
    .await
    .map(|_| ())
}

async fn no_store(request: Request<Body>, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
