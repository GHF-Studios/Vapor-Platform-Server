use crate::pagination::{Page, PageCursor, PageQuery};
use crate::ssh_policy::PENDING_TTL_SECONDS;
use axum::body::Body;
use axum::extract::{Path, Query, State};
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
const MAX_REQUEST_EVENTS: i64 = 100;
const MAX_OPEN_REQUESTS_PER_PROFILE: i64 = 3;
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
    requester_profile_id: String,
    requester_kind: String,
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
struct SshRequestEvent {
    from_status: Option<String>,
    to_status: String,
    actor_profile_id: Option<String>,
    actor_kind: String,
    actor_label: Option<String>,
    at_unix: i64,
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RootOperationResult {
    ok: bool,
    operation_id: String,
    state: String,
    detail: String,
}

fn parse_root_operation_output(
    process_succeeded: bool,
    stdout: &[u8],
    expected_id: &str,
    expected_success_state: &str,
) -> Result<(), String> {
    if stdout.len() > 4096 {
        return Err("privileged helper returned an oversized result".to_owned());
    }
    let result: RootOperationResult = serde_json::from_slice(stdout)
        .map_err(|_| "privileged helper returned an invalid result".to_owned())?;
    if result.operation_id != expected_id
        || result.detail.is_empty()
        || result.detail.len() > 256
        || result.detail.chars().any(char::is_control)
        || (result.ok && (!process_succeeded || result.state != expected_success_state))
        || (!result.ok && (process_succeeded || result.state != "failed"))
    {
        return Err("privileged helper returned an inconsistent result".to_owned());
    }
    if result.ok {
        Ok(())
    } else {
        Err(result.detail)
    }
}

pub(crate) fn router() -> Router {
    Router::new()
        .route("/api/ssh-access/requests", post(create_request))
        .route("/api/admin/ssh-access/requests", get(list_requests))
        .route(
            "/api/admin/ssh-access/requests/{id}/events",
            get(list_request_events),
        )
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
            decided_by_actor_kind TEXT NOT NULL DEFAULT 'identity_profile',
            decided_by_actor_label TEXT,
            decided_at_unix INTEGER,
            operation_detail TEXT
        )",
    )
    .execute(&pool)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_platform_ssh_access_status ON platform_ssh_access_requests(status, created_at_unix)")
        .execute(&pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_platform_ssh_access_created_id ON platform_ssh_access_requests(created_at_unix DESC, id DESC)")
        .execute(&pool)
        .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS platform_ssh_access_events (
            event_id INTEGER PRIMARY KEY AUTOINCREMENT,
            request_id TEXT NOT NULL,
            from_status TEXT,
            to_status TEXT NOT NULL,
            actor_profile_id TEXT,
            actor_kind TEXT NOT NULL DEFAULT 'identity_profile',
            actor_label TEXT,
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
    crate::ssh_schema::ensure_audit_actor_columns(&pool).await?;
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

async fn list_requests(
    State(state): State<SshAccessState>,
    headers: HeaderMap,
    Query(query): Query<PageQuery>,
) -> Response {
    if let Err(response) = require_root(&state, &headers).await {
        return response;
    }
    if expire_stale_requests(&state.pool).await.is_err() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let cursor = match query.decoded_cursor() {
        Ok(cursor) => cursor,
        Err(()) => return (StatusCode::BAD_REQUEST, "invalid pagination cursor\n").into_response(),
    };
    let page_size = query.page_size();
    let rows = match fetch_request_rows(&state.pool, cursor.as_ref(), page_size).await {
        Ok(rows) => rows,
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let page = match Page::from_items(rows, page_size, |row| PageCursor {
        created_at_unix: row.get("created_at_unix"),
        id: row.get("id"),
    }) {
        Ok(page) => page,
        Err(()) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let requests = page
        .items
        .into_iter()
        .filter_map(|row| {
            Some(SshRequestSummary {
                id: row.try_get("id").ok()?,
                requester_profile_id: row.try_get("requester_profile_id").ok()?,
                requester_kind: if row
                    .try_get::<Option<String>, _>("requester_credential_id")
                    .ok()?
                    .is_some()
                {
                    "request credential".to_owned()
                } else {
                    "interactive session".to_owned()
                },
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
    Json(Page {
        items: requests,
        next_cursor: page.next_cursor,
    })
    .into_response()
}

async fn fetch_request_rows(
    pool: &SqlitePool,
    cursor: Option<&PageCursor>,
    page_size: usize,
) -> Result<Vec<sqlx::sqlite::SqliteRow>, sqlx::Error> {
    let base = "SELECT id, requester_profile_id, requester_credential_id, fingerprint, purpose, requested_capabilities_json, granted_capabilities_json,
                requested_duration_seconds, granted_duration_seconds, status, version, created_at_unix, expires_at_unix, operation_detail
                FROM platform_ssh_access_requests";
    let query = if cursor.is_some() {
        format!("{base} WHERE created_at_unix < ? OR (created_at_unix = ? AND id < ?) ORDER BY created_at_unix DESC, id DESC LIMIT ?")
    } else {
        format!("{base} ORDER BY created_at_unix DESC, id DESC LIMIT ?")
    };
    let mut statement = sqlx::query(&query);
    if let Some(cursor) = cursor {
        statement = statement
            .bind(cursor.created_at_unix)
            .bind(cursor.created_at_unix)
            .bind(&cursor.id);
    }
    statement.bind((page_size + 1) as i64).fetch_all(pool).await
}

async fn list_request_events(
    State(state): State<SshAccessState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !valid_id(&id) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if let Err(response) = require_root(&state, &headers).await {
        return response;
    }
    let rows = match sqlx::query(
        "SELECT from_status, to_status, actor_profile_id, actor_kind, actor_label, at_unix
         FROM platform_ssh_access_events WHERE request_id=? ORDER BY event_id DESC LIMIT ?",
    )
    .bind(&id)
    .bind(MAX_REQUEST_EVENTS)
    .fetch_all(&state.pool)
    .await
    {
        Ok(rows) => rows,
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let events = rows
        .into_iter()
        .filter_map(|row| {
            Some(SshRequestEvent {
                from_status: row.try_get("from_status").ok()?,
                to_status: row.try_get("to_status").ok()?,
                actor_profile_id: row.try_get("actor_profile_id").ok()?,
                actor_kind: row.try_get("actor_kind").ok()?,
                actor_label: row.try_get("actor_label").ok()?,
                at_unix: row.try_get("at_unix").ok()?,
            })
        })
        .collect::<Vec<_>>();
    Json(events).into_response()
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
    if expire_stale_requests(&state.pool).await.is_err() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
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
    let now = unix_now();
    let expires = now.saturating_add(request.granted_duration_seconds);
    let granted = serde_json::to_string(&request.granted_capabilities).unwrap_or_default();
    match begin_ssh_grant(
        &state.pool,
        &id,
        request.expected_version,
        &actor,
        &granted,
        request.granted_duration_seconds,
        expires,
        now,
    )
    .await
    {
        Ok(true) => {}
        Ok(false) => return StatusCode::CONFLICT.into_response(),
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
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
    if let Err(helper_failure) = run_root_operation(&payload).await {
        let detail = format!("grant helper failed: {helper_failure}");
        let update = sqlx::query(
            "UPDATE platform_ssh_access_requests SET status = 'failed', version = version + 1,
             operation_detail = ? WHERE id = ? AND status = 'provisioning' AND version = ?
             AND operation_detail = 'granting'",
        )
        .bind(&detail)
        .bind(&id)
        .bind(request.expected_version + 1)
        .execute(&state.pool)
        .await;
        if !matches!(update, Ok(result) if result.rows_affected() == 1) {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "grant helper failed and Platform could not record the result; inspect the host before retrying\n",
            )
                .into_response();
        }
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("{detail}; inspect the request and revoke if any access remains\n"),
        )
            .into_response();
    }
    let active_result = sqlx::query(
        "UPDATE platform_ssh_access_requests SET status = 'active', version = version + 1,
         expires_at_unix = ?,
         operation_detail = NULL WHERE id = ? AND status = 'provisioning' AND version = ?",
    )
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
         public_key='', decided_by_profile_id=?, decided_by_actor_kind='identity_profile', decided_by_actor_label=NULL,
         decided_at_unix=?, operation_detail='request rejected'
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
         decided_by_profile_id = ?, decided_by_actor_kind='identity_profile', decided_by_actor_label=NULL,
         decided_at_unix = ?, operation_detail='revoking'
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
    if let Err(helper_failure) = run_root_operation(&payload).await {
        let detail =
            format!("revocation helper failed; access may remain active: {helper_failure}");
        let update = sqlx::query(
            "UPDATE platform_ssh_access_requests SET status = 'failed', version = version + 1,
             operation_detail = ? WHERE id = ? AND status = 'provisioning' AND version = ?
             AND operation_detail = 'revoking'",
        )
        .bind(&detail)
        .bind(&id)
        .bind(request.expected_version + 1)
        .execute(&state.pool)
        .await;
        if !matches!(update, Ok(result) if result.rows_affected() == 1) {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "revocation helper failed and Platform could not record the result; access may remain active\n",
            )
                .into_response();
        }
        return (StatusCode::SERVICE_UNAVAILABLE, format!("{detail}\n")).into_response();
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

async fn run_root_operation(payload: &RootOperation<'_>) -> Result<(), String> {
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
        .map_err(|_| "could not start privileged helper".to_owned())?;
    let input = serde_json::to_vec(payload)
        .map_err(|_| "could not encode privileged operation".to_owned())?;
    if input.len() > 4096 {
        return Err("privileged operation input exceeds its size limit".to_owned());
    }
    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        stdin
            .write_all(&input)
            .await
            .map_err(|_| "could not send privileged operation".to_owned())?;
    } else {
        return Err("privileged helper input is unavailable".to_owned());
    }
    let output = tokio::time::timeout(Duration::from_secs(10), child.wait_with_output())
        .await
        .map_err(|_| "privileged helper timed out".to_owned())?
        .map_err(|_| "could not collect privileged helper result".to_owned())?;
    let expected_success_state = match payload.operation {
        "dispatch_deploy" => "queued",
        "reconcile_deploy" => "failed",
        "grant_ssh" => "active",
        "revoke_ssh" => "revoked",
        _ => return Err("unsupported privileged operation".to_owned()),
    };
    parse_root_operation_output(
        output.status.success(),
        &output.stdout,
        payload.request_id,
        expected_success_state,
    )
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

async fn begin_ssh_grant(
    pool: &SqlitePool,
    id: &str,
    expected_version: i64,
    actor_profile_id: &str,
    granted_capabilities_json: &str,
    granted_duration_seconds: i64,
    expires_at_unix: i64,
    now: i64,
) -> Result<bool, sqlx::Error> {
    let cutoff = now.saturating_sub(PENDING_TTL_SECONDS);
    let result = sqlx::query(
        "UPDATE platform_ssh_access_requests SET status = 'provisioning', version = version + 1,
         decided_by_profile_id = ?, decided_by_actor_kind='identity_profile', decided_by_actor_label=NULL,
         granted_capabilities_json = ?, granted_duration_seconds = ?, decided_at_unix = ?,
         expires_at_unix = ?, operation_detail='granting'
         WHERE id = ? AND status = 'pending' AND version = ? AND created_at_unix > ?",
    )
    .bind(actor_profile_id)
    .bind(granted_capabilities_json)
    .bind(granted_duration_seconds)
    .bind(now)
    .bind(expires_at_unix)
    .bind(id)
    .bind(expected_version)
    .bind(cutoff)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
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
         public_key='', decided_by_profile_id=NULL, decided_by_actor_kind='system_expiry',
         decided_by_actor_label='expiry-timer', operation_detail=NULL
         WHERE status='pending' AND created_at_unix <= ?",
    )
    .bind(cutoff)
    .execute(pool)
    .await?;
    sqlx::query(
        "UPDATE platform_ssh_access_requests SET status='failed', version=version+1,
         decided_by_actor_kind='system_recovery', decided_by_actor_label='startup-recovery',
         operation_detail='interrupted privileged operation; cleanup may be required'
         WHERE status='provisioning' AND decided_at_unix <= ?
           AND COALESCE(operation_detail, '') NOT IN ('granting', 'revoking', 'expiring')",
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

#[cfg(test)]
mod tests {
    use super::*;

    async fn grant_transition_pool(created_at: i64) -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE platform_ssh_access_requests (
                id TEXT PRIMARY KEY, status TEXT NOT NULL, version INTEGER NOT NULL,
                decided_by_profile_id TEXT, decided_by_actor_kind TEXT,
                decided_by_actor_label TEXT, granted_capabilities_json TEXT,
                granted_duration_seconds INTEGER, decided_at_unix INTEGER,
                expires_at_unix INTEGER, operation_detail TEXT, created_at_unix INTEGER NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO platform_ssh_access_requests (id, status, version, created_at_unix)
             VALUES ('request-1', 'pending', 4, ?)",
        )
        .bind(created_at)
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    #[tokio::test]
    async fn stale_ssh_request_cannot_transition_to_provisioning() {
        let now = PENDING_TTL_SECONDS + 10_000;
        let cutoff = now - PENDING_TTL_SECONDS;
        let pool = grant_transition_pool(cutoff).await;

        let changed = begin_ssh_grant(
            &pool,
            "request-1",
            4,
            "root-profile",
            "[\"vapor_deploy\"]",
            3600,
            now + 3600,
            now,
        )
        .await
        .unwrap();

        assert!(!changed);
        let row = sqlx::query(
            "SELECT status, version FROM platform_ssh_access_requests WHERE id='request-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.get::<String, _>("status"), "pending");
        assert_eq!(row.get::<i64, _>("version"), 4);
    }

    #[test]
    fn root_operation_results_are_bound_and_failure_details_are_bounded() {
        let expected_id = "request-1";
        let success =
            br#"{"ok":true,"operation_id":"request-1","state":"active","detail":"grant active"}"#;
        assert_eq!(
            parse_root_operation_output(true, success, expected_id, "active"),
            Ok(())
        );
        let wrong_state =
            br#"{"ok":true,"operation_id":"request-1","state":"revoked","detail":"grant active"}"#;
        assert!(parse_root_operation_output(true, wrong_state, expected_id, "active").is_err());

        let failure = br#"{"ok":false,"operation_id":"request-1","state":"failed","detail":"rollback incomplete"}"#;
        assert_eq!(
            parse_root_operation_output(false, failure, expected_id, "active"),
            Err("rollback incomplete".to_owned())
        );

        let mismatched_id =
            br#"{"ok":false,"operation_id":"other","state":"failed","detail":"failure"}"#;
        assert!(parse_root_operation_output(false, mismatched_id, expected_id, "active").is_err());
        assert!(
            parse_root_operation_output(false, &vec![b'x'; 4097], expected_id, "active").is_err()
        );
    }

    #[tokio::test]
    async fn current_ssh_request_can_transition_to_provisioning() {
        let now = PENDING_TTL_SECONDS + 10_000;
        let pool = grant_transition_pool(now - PENDING_TTL_SECONDS + 1).await;

        assert!(begin_ssh_grant(
            &pool,
            "request-1",
            4,
            "root-profile",
            "[\"vapor_deploy\"]",
            3600,
            now + 3600,
            now,
        )
        .await
        .unwrap());
    }

    #[tokio::test]
    async fn ssh_request_pages_are_stable_when_timestamps_tie() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("CREATE TABLE platform_ssh_access_requests (id TEXT, requester_profile_id TEXT, requester_credential_id TEXT, fingerprint TEXT, purpose TEXT, requested_capabilities_json TEXT, granted_capabilities_json TEXT, requested_duration_seconds INTEGER, granted_duration_seconds INTEGER, status TEXT, version INTEGER, created_at_unix INTEGER, expires_at_unix INTEGER, operation_detail TEXT)")
            .execute(&pool).await.unwrap();
        for id in ["a", "b", "c"] {
            sqlx::query(
                "INSERT INTO platform_ssh_access_requests (id, created_at_unix) VALUES (?, 100)",
            )
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
        }

        let first = fetch_request_rows(&pool, None, 2).await.unwrap();
        let first = Page::from_items(first, 2, |row| PageCursor {
            created_at_unix: row.get("created_at_unix"),
            id: row.get("id"),
        })
        .unwrap();
        assert_eq!(
            first
                .items
                .iter()
                .map(|row| row.get::<String, _>("id"))
                .collect::<Vec<_>>(),
            vec!["c", "b"]
        );

        let cursor = PageCursor::decode(first.next_cursor.as_deref().unwrap()).unwrap();
        let second = fetch_request_rows(&pool, Some(&cursor), 2).await.unwrap();
        let second = Page::from_items(second, 2, |row| PageCursor {
            created_at_unix: row.get("created_at_unix"),
            id: row.get("id"),
        })
        .unwrap();
        assert_eq!(
            second
                .items
                .iter()
                .map(|row| row.get::<String, _>("id"))
                .collect::<Vec<_>>(),
            vec!["a"]
        );
        assert!(second.next_cursor.is_none());
    }

    #[tokio::test]
    async fn request_history_requires_an_identity_root_session() {
        let state = SshAccessState {
            pool: SqlitePoolOptions::new()
                .max_connections(1)
                .connect_lazy("sqlite::memory:")
                .unwrap(),
            client: reqwest::Client::new(),
            identity_root_authz_url: String::new(),
            identity_root_authz_token: None,
            public_origin: "https://example.invalid".to_owned(),
            ssh_host: "example.invalid".to_owned(),
        };
        let response = list_request_events(
            State(state),
            HeaderMap::new(),
            Path("fb9ef006-f6ad-420c-9ad7-31337b2b1e52".to_owned()),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
