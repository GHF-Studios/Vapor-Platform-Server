use axum::body::Body;
use axum::http::header::{CACHE_CONTROL, COOKIE, SET_COOKIE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::path::PathBuf;
use std::time::Duration;

const SESSION_SECONDS: i64 = 20 * 60;
const COOKIE_NAME: &str = "vapor_emergency_console";
const COOKIE_PATH: &str = "/emergency-terminal/";
const GATEWAY_MARKER: &str = "CaddyEmergencyTerminalV1";
static LIFECYCLE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone)]
struct ConsoleState {
    pool: SqlitePool,
    client: reqwest::Client,
    identity_authz_url: String,
    identity_authz_token: Option<String>,
    public_origin: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArmRequest {
    acknowledge_root_shell: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfirmRequest {
    confirmed: bool,
}

#[derive(Serialize)]
struct ConsoleEvent {
    event: String,
    at_unix: i64,
}

#[derive(Serialize)]
struct ConsoleStatus {
    enabled: bool,
    configured: bool,
    active: bool,
    session_id: Option<String>,
    expires_at_unix: Option<i64>,
    connected: bool,
    events: Vec<ConsoleEvent>,
}

#[derive(Serialize)]
struct ArmedSession {
    session_id: String,
    expires_at_unix: i64,
    terminal_path: &'static str,
}

pub(crate) fn router() -> Router {
    Router::new()
        .route("/api/admin/emergency-console/status", get(status))
        .route("/api/admin/emergency-console/enable", post(enable))
        .route("/api/admin/emergency-console/disable", post(disable))
        .route("/api/admin/emergency-console/session", post(arm))
        .route(
            "/api/admin/emergency-console/session/end",
            post(end_session),
        )
        .route(
            "/api/admin/emergency-console/authorize",
            get(authorize_terminal),
        )
        .with_state(state())
        .route_layer(axum::middleware::from_fn(no_store))
}

pub(crate) async fn initialize() -> Result<(), Box<dyn std::error::Error>> {
    let directory = state_directory();
    tokio::fs::create_dir_all(directory).await?;
    let pool = create_pool();
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS platform_emergency_console_config (
            slot INTEGER PRIMARY KEY CHECK(slot = 1),
            enabled INTEGER NOT NULL CHECK(enabled IN (0,1))
        )",
    )
    .execute(&pool)
    .await?;
    sqlx::query(
        "INSERT OR IGNORE INTO platform_emergency_console_config(slot, enabled) VALUES (1,0)",
    )
    .execute(&pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS platform_emergency_console_sessions (
            id TEXT PRIMARY KEY,
            cookie_sha256 TEXT NOT NULL,
            identity_session_sha256 TEXT NOT NULL,
            profile_id TEXT NOT NULL,
            status TEXT NOT NULL CHECK(status IN ('active','ended','revoked','expired','start_failed')),
            created_at_unix INTEGER NOT NULL,
            expires_at_unix INTEGER NOT NULL CHECK(expires_at_unix > created_at_unix),
            connected_at_unix INTEGER,
            ended_at_unix INTEGER
        )",
    )
    .execute(&pool)
    .await?;
    sqlx::query(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_platform_emergency_console_one_active
         ON platform_emergency_console_sessions(status) WHERE status = 'active'",
    )
    .execute(&pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_platform_emergency_console_created
         ON platform_emergency_console_sessions(created_at_unix DESC)",
    )
    .execute(&pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS platform_emergency_console_events (
            event_id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT,
            actor_profile_id TEXT,
            event TEXT NOT NULL CHECK(event IN ('enabled','disabled','started','start_failed','connected','ended','revoked','expired','stop_failed')),
            at_unix INTEGER NOT NULL
        )",
    )
    .execute(&pool)
    .await?;
    let expiry_pool = pool.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            if expire_sessions(&expiry_pool, unix_now()).await.is_err() {
                eprintln!("emergency console expiry sweep failed");
            }
        }
    });
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
    SqlitePoolOptions::new()
        .max_connections(5)
        .connect_lazy_with(
            SqliteConnectOptions::new()
                .filename(database_path)
                .create_if_missing(true)
                .foreign_keys(true)
                .busy_timeout(Duration::from_secs(5))
                .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal),
        )
}

fn state() -> ConsoleState {
    let domain =
        std::env::var("VAPOR_DOMAIN").unwrap_or_else(|_| "vapor.ghf-studios.site".to_owned());
    ConsoleState {
        pool: create_pool(),
        client: reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(1))
            .timeout(Duration::from_secs(3))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("valid emergency-console authorization client"),
        identity_authz_url: std::env::var("VAPOR_PLATFORM_IDENTITY_AUTHZ_URL").unwrap_or_else(
            |_| "http://127.0.0.1:7110/api/identity/v1/internal/authorization/check".to_owned(),
        ),
        identity_authz_token: std::env::var("VAPOR_PLATFORM_IDENTITY_AUTHZ_TOKEN").ok(),
        public_origin: std::env::var("VAPOR_PLATFORM_PUBLIC_ORIGIN")
            .unwrap_or_else(|_| format!("https://{domain}")),
    }
}

async fn status(state: axum::extract::State<ConsoleState>, headers: HeaderMap) -> Response {
    let (state, profile_id, identity_token) = match authorized_profile(&state.0, &headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let now = unix_now();
    if !configured_enabled() {
        let _lifecycle = LIFECYCLE_LOCK.lock().await;
        // Stop even if the state write failed: the host flag already denies
        // authorization and this clears any orphaned backend process.
        let state_disabled = disable_internal(&state, &profile_id, now).await.is_ok();
        let backend_stopped = helper_action("emergency-console-stop", &identity_token)
            .await
            .is_ok();
        if !state_disabled || !backend_stopped {
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "console_disable_incomplete",
            );
        }
    }
    if expire_sessions(&state.pool, now).await.is_err() {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "console_status_unavailable",
        );
    }
    let config_enabled = match sqlx::query_scalar::<_, i64>(
        "SELECT enabled FROM platform_emergency_console_config WHERE slot=1",
    )
    .fetch_one(&state.pool)
    .await
    {
        Ok(value) => value == 1,
        Err(_) => {
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "console_status_unavailable",
            );
        }
    };
    let active = sqlx::query(
        "SELECT id, expires_at_unix, connected_at_unix FROM platform_emergency_console_sessions
         WHERE status='active' AND expires_at_unix>? ORDER BY created_at_unix DESC LIMIT 1",
    )
    .bind(now)
    .fetch_optional(&state.pool)
    .await;
    let active = match active {
        Ok(row) => row,
        Err(_) => {
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "console_status_unavailable",
            );
        }
    };
    let events = sqlx::query(
        "SELECT event, at_unix FROM platform_emergency_console_events ORDER BY event_id DESC LIMIT 20",
    )
    .fetch_all(&state.pool)
    .await;
    let events = match events {
        Ok(rows) => rows
            .into_iter()
            .map(|row| ConsoleEvent {
                event: row.get("event"),
                at_unix: row.get("at_unix"),
            })
            .collect(),
        Err(_) => {
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "console_status_unavailable",
            );
        }
    };
    private_json(
        StatusCode::OK,
        ConsoleStatus {
            enabled: configured_enabled() && config_enabled,
            configured: configured_enabled(),
            active: active.is_some(),
            session_id: active.as_ref().map(|row| row.get("id")),
            expires_at_unix: active.as_ref().map(|row| row.get("expires_at_unix")),
            connected: active
                .as_ref()
                .is_some_and(|row| row.get::<Option<i64>, _>("connected_at_unix").is_some()),
            events,
        },
    )
}

async fn enable(
    state: axum::extract::State<ConsoleState>,
    headers: HeaderMap,
    Json(request): Json<ConfirmRequest>,
) -> Response {
    let (state, profile_id, _identity_token) = match authorized_profile(&state.0, &headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    if !same_origin(&state, &headers) || !request.confirmed {
        return error(StatusCode::FORBIDDEN, "same_origin_confirmation_required");
    }
    let _lifecycle = LIFECYCLE_LOCK.lock().await;
    if !configured_enabled() {
        return error(StatusCode::CONFLICT, "console_not_enabled_in_host_config");
    }
    let mut transaction = match state.pool.begin().await {
        Ok(transaction) => transaction,
        Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE, "console_state_unavailable"),
    };
    let enabled =
        sqlx::query("UPDATE platform_emergency_console_config SET enabled=1 WHERE slot=1")
            .execute(&mut *transaction)
            .await;
    if !enabled.is_ok_and(|result| result.rows_affected() == 1) {
        return error(StatusCode::SERVICE_UNAVAILABLE, "console_state_unavailable");
    }
    if record_event_tx(
        &mut transaction,
        None,
        Some(&profile_id),
        "enabled",
        unix_now(),
    )
    .await
    .is_err()
    {
        return error(StatusCode::SERVICE_UNAVAILABLE, "console_audit_unavailable");
    }
    if transaction.commit().await.is_err() {
        return error(StatusCode::SERVICE_UNAVAILABLE, "console_state_unavailable");
    }
    private_json(StatusCode::OK, serde_json::json!({"enabled": true}))
}

async fn disable(
    state: axum::extract::State<ConsoleState>,
    headers: HeaderMap,
    Json(request): Json<ConfirmRequest>,
) -> Response {
    let (state, profile_id, identity_token) = match authorized_profile(&state.0, &headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    if !same_origin(&state, &headers) || !request.confirmed {
        return error(StatusCode::FORBIDDEN, "same_origin_confirmation_required");
    }
    let _lifecycle = LIFECYCLE_LOCK.lock().await;
    let now = unix_now();
    let state_disabled = disable_internal(&state, &profile_id, now).await.is_ok();
    let backend_stopped = helper_action("emergency-console-stop", &identity_token)
        .await
        .is_ok();
    if !state_disabled || !backend_stopped {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "console_disabled_but_stop_unconfirmed",
        );
    }
    private_json(StatusCode::OK, serde_json::json!({"enabled": false}))
}

async fn arm(
    state: axum::extract::State<ConsoleState>,
    headers: HeaderMap,
    Json(request): Json<ArmRequest>,
) -> Response {
    let (state, profile_id, _identity_token) = match authorized_profile(&state.0, &headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    if !same_origin(&state, &headers) || !request.acknowledge_root_shell {
        return error(
            StatusCode::FORBIDDEN,
            "same_origin_root_shell_acknowledgement_required",
        );
    }
    let _lifecycle = LIFECYCLE_LOCK.lock().await;
    if !configured_enabled() {
        return error(StatusCode::NOT_FOUND, "console_not_configured");
    }
    let enabled = sqlx::query_scalar::<_, i64>(
        "SELECT enabled FROM platform_emergency_console_config WHERE slot=1",
    )
    .fetch_one(&state.pool)
    .await;
    if !matches!(enabled, Ok(1)) {
        return error(StatusCode::CONFLICT, "console_disabled");
    }
    let now = unix_now();
    let expires = now.saturating_add(SESSION_SECONDS);
    let session_id = uuid::Uuid::new_v4().to_string();
    let secret = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let identity_token = crate::admin_auth::session_token(&headers).unwrap_or_default();
    let cookie_hash = sha256_hex(secret.as_bytes());
    let identity_session_hash = sha256_hex(identity_token.as_bytes());
    if expire_sessions(&state.pool, now).await.is_err() {
        return error(StatusCode::SERVICE_UNAVAILABLE, "console_state_unavailable");
    }
    let mut transaction = match state.pool.begin().await {
        Ok(transaction) => transaction,
        Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE, "console_state_unavailable"),
    };
    let still_enabled = sqlx::query_scalar::<_, i64>(
        "SELECT enabled FROM platform_emergency_console_config WHERE slot=1",
    )
    .fetch_one(&mut *transaction)
    .await;
    if !matches!(still_enabled, Ok(1)) || !configured_enabled() {
        return error(StatusCode::CONFLICT, "console_disabled");
    }
    let inserted = sqlx::query(
        "INSERT INTO platform_emergency_console_sessions
         (id,cookie_sha256,identity_session_sha256,profile_id,status,created_at_unix,expires_at_unix)
         VALUES (?,?,?,?,'active',?,?)",
    )
    .bind(&session_id)
    .bind(&cookie_hash)
    .bind(&identity_session_hash)
    .bind(&profile_id)
    .bind(now)
    .bind(expires)
    .execute(&mut *transaction)
    .await;
    if inserted.is_err() {
        return error(StatusCode::CONFLICT, "another_console_session_is_active");
    }
    if record_event_tx(
        &mut transaction,
        Some(&session_id),
        Some(&profile_id),
        "started",
        now,
    )
    .await
    .is_err()
    {
        return error(StatusCode::SERVICE_UNAVAILABLE, "console_audit_unavailable");
    }
    if transaction.commit().await.is_err() {
        return error(StatusCode::SERVICE_UNAVAILABLE, "console_state_unavailable");
    }
    if helper_action("emergency-console-start", &identity_token)
        .await
        .is_err()
    {
        let _ = helper_action("emergency-console-stop", &identity_token).await;
        let _ =
            finish_session_start_failure(&state.pool, &session_id, &profile_id, unix_now()).await;
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "console_backend_unavailable",
        );
    }
    // Disable may race the service start after the DB transition. Revalidate
    // before returning the one-time cookie; if revocation won, stop the shell.
    let grant_still_active = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM platform_emergency_console_sessions s
         JOIN platform_emergency_console_config c ON c.slot=1
         WHERE s.id=? AND s.status='active' AND s.expires_at_unix>? AND c.enabled=1",
    )
    .bind(&session_id)
    .bind(unix_now())
    .fetch_one(&state.pool)
    .await
    .is_ok_and(|count| count == 1);
    if !grant_still_active {
        let _ = helper_action("emergency-console-stop", &identity_token).await;
        let _ = transition_active_session(
            &state.pool,
            &session_id,
            &profile_id,
            "revoked",
            "revoked",
            unix_now(),
        )
        .await;
        return error(StatusCode::CONFLICT, "console_session_revoked");
    }
    let cookie = format!(
        "{COOKIE_NAME}={secret}; Path={COOKIE_PATH}; Max-Age={SESSION_SECONDS}; Secure; HttpOnly; SameSite=Strict"
    );
    let mut response = private_json(
        StatusCode::CREATED,
        ArmedSession {
            session_id,
            expires_at_unix: expires,
            terminal_path: "/emergency-terminal/",
        },
    );
    match HeaderValue::from_str(&cookie) {
        Ok(cookie) => {
            response.headers_mut().insert(SET_COOKIE, cookie);
            response
        }
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "console_cookie_unavailable",
        ),
    }
}

async fn end_session(
    state: axum::extract::State<ConsoleState>,
    headers: HeaderMap,
    Json(request): Json<ConfirmRequest>,
) -> Response {
    let (state, profile_id, identity_token) = match authorized_profile(&state.0, &headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    if !same_origin(&state, &headers) || !request.confirmed {
        return error(StatusCode::FORBIDDEN, "same_origin_confirmation_required");
    }
    let _lifecycle = LIFECYCLE_LOCK.lock().await;
    let mut transaction = match state.pool.begin().await {
        Ok(transaction) => transaction,
        Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE, "console_state_unavailable"),
    };
    let session_id = sqlx::query_scalar::<_, String>(
        "SELECT id FROM platform_emergency_console_sessions WHERE status='active' ORDER BY created_at_unix DESC LIMIT 1",
    )
    .fetch_optional(&mut *transaction)
    .await;
    let session_id = match session_id {
        Ok(value) => value,
        Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE, "console_state_unavailable"),
    };
    if let Some(session_id) = session_id.as_deref() {
        let updated = sqlx::query(
            "UPDATE platform_emergency_console_sessions SET status='ended', ended_at_unix=?
             WHERE id=? AND status='active'",
        )
        .bind(unix_now())
        .bind(session_id)
        .execute(&mut *transaction)
        .await;
        if !updated.is_ok_and(|result| result.rows_affected() == 1) {
            return error(StatusCode::SERVICE_UNAVAILABLE, "console_state_unavailable");
        }
        if record_event_tx(
            &mut transaction,
            Some(session_id),
            Some(&profile_id),
            "ended",
            unix_now(),
        )
        .await
        .is_err()
        {
            return error(StatusCode::SERVICE_UNAVAILABLE, "console_audit_unavailable");
        }
    }
    if transaction.commit().await.is_err() {
        return error(StatusCode::SERVICE_UNAVAILABLE, "console_state_unavailable");
    }
    let stopped = helper_action("emergency-console-stop", &identity_token)
        .await
        .is_ok();
    if !stopped {
        if let Some(session_id) = session_id.as_deref() {
            let _ = record_event(
                &state.pool,
                Some(session_id),
                Some(&profile_id),
                "stop_failed",
                unix_now(),
            )
            .await;
        }
    }
    if !stopped {
        return clear_cookie(error(
            StatusCode::SERVICE_UNAVAILABLE,
            "console_ended_but_stop_unconfirmed",
        ));
    }
    clear_cookie(private_json(
        StatusCode::OK,
        serde_json::json!({"ended": true}),
    ))
}

async fn authorize_terminal(
    axum::extract::State(state): axum::extract::State<ConsoleState>,
    headers: HeaderMap,
) -> Response {
    if !configured_enabled() || !gateway_request(&headers) {
        return error(StatusCode::NOT_FOUND, "console_unavailable");
    }
    let enabled = sqlx::query_scalar::<_, i64>(
        "SELECT enabled FROM platform_emergency_console_config WHERE slot=1",
    )
    .fetch_one(&state.pool)
    .await;
    if !matches!(enabled, Ok(1)) {
        return error(StatusCode::NOT_FOUND, "console_unavailable");
    }
    let Some(identity_token) = crate::admin_auth::session_token(&headers) else {
        return error(StatusCode::UNAUTHORIZED, "sign_in_required");
    };
    let Some(cookie_token) = cookie_value(&headers, COOKIE_NAME) else {
        return error(StatusCode::UNAUTHORIZED, "console_grant_required");
    };
    let profile_id = match crate::admin_auth::authorize_root_profile_with_client(
        &headers,
        state.identity_authz_token.as_deref(),
        &state.identity_authz_url,
        &state.client,
    )
    .await
    {
        Ok(profile) => profile,
        Err(failure) => return error(failure.status(), "root_identity_required"),
    };
    if expire_sessions(&state.pool, unix_now()).await.is_err() {
        return error(StatusCode::SERVICE_UNAVAILABLE, "console_state_unavailable");
    }
    let cookie_hash = sha256_hex(cookie_token.as_bytes());
    let identity_hash = sha256_hex(identity_token.as_bytes());
    let now = unix_now();
    let session = sqlx::query(
        "SELECT id, profile_id, connected_at_unix FROM platform_emergency_console_sessions
         WHERE cookie_sha256=? AND identity_session_sha256=? AND status='active' AND expires_at_unix>? LIMIT 1",
    )
    .bind(cookie_hash)
    .bind(identity_hash)
    .bind(now)
    .fetch_optional(&state.pool)
    .await;
    let session = match session {
        Ok(Some(session)) if session.get::<String, _>("profile_id") == profile_id => session,
        _ => return error(StatusCode::UNAUTHORIZED, "console_grant_invalid_or_expired"),
    };
    let is_websocket = headers
        .get("x-vapor-console-original-upgrade")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket"));
    if is_websocket {
        let origin = headers
            .get("x-vapor-console-original-origin")
            .and_then(|value| value.to_str().ok());
        let fetch_site = headers
            .get("x-vapor-console-original-fetch-site")
            .and_then(|value| value.to_str().ok());
        if origin != Some(state.public_origin.as_str()) || fetch_site != Some("same-origin") {
            return error(StatusCode::FORBIDDEN, "same_origin_websocket_required");
        }
        if session.get::<Option<i64>, _>("connected_at_unix").is_some() {
            return error(StatusCode::CONFLICT, "console_session_already_connected");
        }
        let connected = sqlx::query(
            "UPDATE platform_emergency_console_sessions SET connected_at_unix=?
             WHERE id=? AND status='active' AND connected_at_unix IS NULL AND expires_at_unix>?",
        )
        .bind(now)
        .bind(session.get::<String, _>("id"))
        .bind(now)
        .execute(&state.pool)
        .await;
        if !connected.is_ok_and(|result| result.rows_affected() == 1) {
            return error(StatusCode::CONFLICT, "console_session_already_connected");
        }
        if record_event(
            &state.pool,
            Some(session.get::<String, _>("id").as_str()),
            Some(&profile_id),
            "connected",
            now,
        )
        .await
        .is_err()
        {
            let _ = sqlx::query(
                "UPDATE platform_emergency_console_sessions SET connected_at_unix=NULL
                 WHERE id=? AND status='active'",
            )
            .bind(session.get::<String, _>("id"))
            .execute(&state.pool)
            .await;
            return error(StatusCode::SERVICE_UNAVAILABLE, "console_audit_unavailable");
        }
    }
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::NO_CONTENT;
    response.headers_mut().insert(
        "x-vapor-console-user",
        HeaderValue::from_static("root-operator"),
    );
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    response
}

async fn authorized_profile(
    state: &ConsoleState,
    headers: &HeaderMap,
) -> Result<(ConsoleState, String, String), Response> {
    match crate::admin_auth::authorize_root_profile_with_client(
        headers,
        state.identity_authz_token.as_deref(),
        &state.identity_authz_url,
        &state.client,
    )
    .await
    {
        Ok(profile) => Ok((
            state.clone(),
            profile,
            crate::admin_auth::session_token(headers)
                .ok_or_else(|| error(StatusCode::UNAUTHORIZED, "sign_in_required"))?
                .to_owned(),
        )),
        Err(failure) => Err(error(failure.status(), "root_identity_required")),
    }
}

async fn disable_internal(
    state: &ConsoleState,
    profile_id: &str,
    now: i64,
) -> Result<Vec<String>, ()> {
    let mut transaction = state.pool.begin().await.map_err(|_| ())?;
    let was_enabled = sqlx::query_scalar::<_, i64>(
        "SELECT enabled FROM platform_emergency_console_config WHERE slot=1",
    )
    .fetch_one(&mut *transaction)
    .await
    .map_err(|_| ())?
        == 1;
    sqlx::query("UPDATE platform_emergency_console_config SET enabled=0 WHERE slot=1")
        .execute(&mut *transaction)
        .await
        .map_err(|_| ())?;
    let active_ids = sqlx::query_scalar::<_, String>(
        "SELECT id FROM platform_emergency_console_sessions WHERE status='active'",
    )
    .fetch_all(&mut *transaction)
    .await
    .map_err(|_| ())?;
    sqlx::query(
        "UPDATE platform_emergency_console_sessions SET status='revoked', ended_at_unix=? WHERE status='active'",
    )
    .bind(now)
    .execute(&mut *transaction)
    .await
    .map_err(|_| ())?;
    if was_enabled || !active_ids.is_empty() {
        record_event_tx(&mut transaction, None, Some(profile_id), "disabled", now)
            .await
            .map_err(|_| ())?;
    }
    for id in &active_ids {
        record_event_tx(
            &mut transaction,
            Some(id.as_str()),
            Some(profile_id),
            "revoked",
            now,
        )
        .await
        .map_err(|_| ())?;
    }
    transaction.commit().await.map_err(|_| ())?;
    Ok(active_ids)
}

/// Expire grants exactly once and persist matching audit events in the same
/// SQLite transaction as the state transition.
async fn expire_sessions(pool: &SqlitePool, now: i64) -> Result<(), ()> {
    let mut transaction = pool.begin().await.map_err(|_| ())?;
    let expired = sqlx::query(
        "SELECT id, profile_id FROM platform_emergency_console_sessions
         WHERE status='active' AND expires_at_unix<=?",
    )
    .bind(now)
    .fetch_all(&mut *transaction)
    .await
    .map_err(|_| ())?;
    if expired.is_empty() {
        transaction.commit().await.map_err(|_| ())?;
        return Ok(());
    }
    sqlx::query(
        "UPDATE platform_emergency_console_sessions SET status='expired', ended_at_unix=?
         WHERE status='active' AND expires_at_unix<=?",
    )
    .bind(now)
    .bind(now)
    .execute(&mut *transaction)
    .await
    .map_err(|_| ())?;
    for row in expired {
        sqlx::query(
            "INSERT INTO platform_emergency_console_events(session_id,actor_profile_id,event,at_unix)
             VALUES (?,?, 'expired', ?)",
        )
        .bind(row.get::<String, _>("id"))
        .bind(row.get::<String, _>("profile_id"))
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(|_| ())?;
    }
    transaction.commit().await.map_err(|_| ())
}

async fn helper_action(action: &'static str, session_token: &str) -> Result<(), ()> {
    let operation = match action {
        "emergency-console-start" => "emergency_console_start",
        "emergency-console-stop" => "emergency_console_stop",
        _ => return Err(()),
    };
    let result: crate::platform_ops_client::OperationResult =
        crate::platform_ops_client::request(&serde_json::json!({
            "operation":operation,
            "session_token":session_token
        }))
        .await
        .map_err(|_| ())?;
    let expected_state = if operation == "emergency_console_start" {
        "started"
    } else {
        "stopped"
    };
    (result.ok
        && result.operation_id == "emergency-console"
        && result.state == expected_state
        && !result.detail.is_empty()
        && result.detail.len() <= 256
        && !result.detail.chars().any(char::is_control))
    .then_some(())
    .ok_or(())
}

async fn record_event(
    pool: &SqlitePool,
    session_id: Option<&str>,
    profile_id: Option<&str>,
    event: &str,
    at_unix: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO platform_emergency_console_events(session_id,actor_profile_id,event,at_unix)
         VALUES (?,?,?,?)",
    )
    .bind(session_id)
    .bind(profile_id)
    .bind(event)
    .bind(at_unix)
    .execute(pool)
    .await
    .map(|_| ())
}

async fn record_event_tx(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    session_id: Option<&str>,
    profile_id: Option<&str>,
    event: &str,
    at_unix: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO platform_emergency_console_events(session_id,actor_profile_id,event,at_unix)
         VALUES (?,?,?,?)",
    )
    .bind(session_id)
    .bind(profile_id)
    .bind(event)
    .bind(at_unix)
    .execute(&mut **transaction)
    .await
    .map(|_| ())
}

async fn finish_session_start_failure(
    pool: &SqlitePool,
    session_id: &str,
    profile_id: &str,
    now: i64,
) -> Result<(), ()> {
    let mut transaction = pool.begin().await.map_err(|_| ())?;
    sqlx::query(
        "UPDATE platform_emergency_console_sessions SET status='start_failed', ended_at_unix=?
         WHERE id=? AND status='active'",
    )
    .bind(now)
    .bind(session_id)
    .execute(&mut *transaction)
    .await
    .map_err(|_| ())?;
    record_event_tx(
        &mut transaction,
        Some(session_id),
        Some(profile_id),
        "start_failed",
        now,
    )
    .await
    .map_err(|_| ())?;
    transaction.commit().await.map_err(|_| ())
}

async fn transition_active_session(
    pool: &SqlitePool,
    session_id: &str,
    profile_id: &str,
    status: &str,
    event: &str,
    now: i64,
) -> Result<(), ()> {
    let mut transaction = pool.begin().await.map_err(|_| ())?;
    let updated = sqlx::query(
        "UPDATE platform_emergency_console_sessions SET status=?, ended_at_unix=?
         WHERE id=? AND status='active'",
    )
    .bind(status)
    .bind(now)
    .bind(session_id)
    .execute(&mut *transaction)
    .await
    .map_err(|_| ())?;
    if updated.rows_affected() == 1 {
        record_event_tx(
            &mut transaction,
            Some(session_id),
            Some(profile_id),
            event,
            now,
        )
        .await
        .map_err(|_| ())?;
    }
    transaction.commit().await.map_err(|_| ())
}

async fn no_store(request: axum::http::Request<Body>, next: axum::middleware::Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    response
}

fn gateway_request(headers: &HeaderMap) -> bool {
    headers
        .get("x-vapor-console-gateway")
        .and_then(|value| value.to_str().ok())
        == Some(GATEWAY_MARKER)
        && headers
            .get("x-vapor-console-original-method")
            .and_then(|value| value.to_str().ok())
            == Some("GET")
        && headers
            .get("x-vapor-console-original-uri")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|uri| {
                !uri.contains('?')
                    && (uri == "/emergency-terminal/" || uri.starts_with("/emergency-terminal/"))
            })
}

fn configured_enabled() -> bool {
    matches!(
        std::env::var("VAPOR_EMERGENCY_CONSOLE_ENABLED").as_deref(),
        Ok("1" | "true")
    )
}

fn same_origin(state: &ConsoleState, headers: &HeaderMap) -> bool {
    headers.get("origin").and_then(|value| value.to_str().ok())
        == Some(state.public_origin.as_str())
}

fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    let cookie = headers.get(COOKIE)?.to_str().ok()?;
    cookie.split(';').find_map(|part| {
        let (key, value) = part.trim().split_once('=')?;
        (key == name && value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .then_some(value)
    })
}

fn sha256_hex(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn error(status: StatusCode, code: &'static str) -> Response {
    private_json(status, serde_json::json!({"error": code}))
}

fn clear_cookie(mut response: Response) -> Response {
    if let Ok(value) = HeaderValue::from_str(&format!(
        "{COOKIE_NAME}=; Path={COOKIE_PATH}; Max-Age=0; Secure; HttpOnly; SameSite=Strict"
    )) {
        response.headers_mut().insert(SET_COOKIE, value);
    }
    response
}

fn private_json<T: Serialize>(status: StatusCode, body: T) -> Response {
    let mut response = (status, Json(body)).into_response();
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    response
}
