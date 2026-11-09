use crate::server_operation_schema::{local_node_id, ServerAction, ServiceId};
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::header::CACHE_CONTROL;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::path::PathBuf;
use std::time::Duration;

const DEFAULT_IDENTITY_AUTHZ_URL: &str =
    "http://127.0.0.1:7110/api/identity/v1/internal/authorization/check";
const MAX_RESULT_DETAIL_BYTES: usize = 240;
const REBOOT_RECONCILIATION_GRACE_SECONDS: i64 = 300;

#[derive(Clone)]
struct OperationsState {
    pool: SqlitePool,
    authz_endpoint: String,
    service_token: Option<String>,
    client: reqwest::Client,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateOperationRequest {
    client_request_id: String,
    target_node_id: String,
    action: serde_json::Value,
    confirmed: bool,
}

#[derive(Serialize)]
struct OperationRecord {
    id: String,
    target_node_id: String,
    action: serde_json::Value,
    state: String,
    version: i64,
    actor_profile_id: String,
    created_at_unix: i64,
    started_at_unix: Option<i64>,
    finished_at_unix: Option<i64>,
    result_detail: Option<String>,
}

#[derive(Serialize)]
struct OperationEvent {
    event: String,
    state: String,
    actor_profile_id: String,
    at_unix: i64,
    detail: Option<String>,
}

#[derive(Serialize)]
struct OperationDetail {
    operation: OperationRecord,
    events: Vec<OperationEvent>,
}

#[derive(Serialize)]
struct OperationList {
    local_node_id: String,
    items: Vec<OperationRecord>,
}

#[derive(Serialize)]
struct PlatformOperationRequest<'a> {
    operation: &'static str,
    request_id: &'a str,
    expected_version: i64,
    session_token: &'a str,
    target_node_id: &'a str,
    action: &'a ServerAction,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OperationState {
    Accepted,
    Running,
    AwaitingReboot,
    Succeeded,
    Failed,
    OutcomeUnknown,
}

impl OperationState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Running => "running",
            Self::AwaitingReboot => "awaiting_reboot",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::OutcomeUnknown => "outcome_unknown",
        }
    }
}

fn allowed_transition(from: OperationState, to: OperationState) -> bool {
    matches!(
        (from, to),
        (OperationState::Accepted, OperationState::Running)
            | (OperationState::Accepted, OperationState::Failed)
            | (OperationState::Running, OperationState::AwaitingReboot)
            | (OperationState::Running, OperationState::Succeeded)
            | (OperationState::Running, OperationState::Failed)
            | (OperationState::Running, OperationState::OutcomeUnknown)
            | (OperationState::AwaitingReboot, OperationState::Succeeded)
            | (OperationState::AwaitingReboot, OperationState::Failed)
            | (
                OperationState::AwaitingReboot,
                OperationState::OutcomeUnknown
            )
            | (OperationState::OutcomeUnknown, OperationState::Succeeded)
            | (OperationState::OutcomeUnknown, OperationState::Failed)
    )
}

fn accepted_operation_can_resume(state: &str) -> bool {
    state == OperationState::Accepted.as_str()
}

fn reboot_resolution(
    before: Option<&str>,
    current: Option<&str>,
    requested_at: Option<i64>,
    now: i64,
) -> Option<(OperationState, &'static str, &'static str)> {
    if before
        .zip(current)
        .is_some_and(|(before, current)| before != current)
    {
        Some((
            OperationState::Succeeded,
            "succeeded",
            "host returned with a different boot ID; reboot completed",
        ))
    } else if requested_at
        .is_some_and(|at| now.saturating_sub(at) >= REBOOT_RECONCILIATION_GRACE_SECONDS)
    {
        Some((
            OperationState::OutcomeUnknown,
            "outcome_unknown",
            "no changed boot ID was observed within five minutes; inspect the host before retrying",
        ))
    } else {
        None
    }
}

pub(crate) fn router() -> Router {
    router_with_state(state())
}

fn router_with_state(state: OperationsState) -> Router {
    Router::new()
        .route(
            "/api/admin/server-operations",
            get(list_operations).post(create_operation),
        )
        .route("/api/admin/server-operations/{id}", get(get_operation))
        .with_state(state)
        .layer(DefaultBodyLimit::max(4096))
        .route_layer(axum::middleware::from_fn(no_store))
}

pub(crate) async fn initialize() -> Result<(), Box<dyn std::error::Error>> {
    let pool = create_pool();
    initialize_pool(&pool).await?;

    // A request lost while its helper was running cannot be retried safely.
    // Preserve its record and make the ambiguous outcome visible to the owner.
    let interrupted = sqlx::query(
        "SELECT id,actor_profile_id FROM platform_server_operations WHERE state='running'",
    )
    .fetch_all(&pool)
    .await?;
    let now = unix_now();
    for row in interrupted {
        let id: String = row.get("id");
        let actor: String = row.get("actor_profile_id");
        let mut transaction = pool.begin().await?;
        let changed = sqlx::query(
            "UPDATE platform_server_operations SET state='outcome_unknown', version=version+1,
                    finished_at_unix=?, result_detail='Platform restarted before recording the helper result; inspect the host before taking another action.'
             WHERE id=? AND state='running'",
        )
        .bind(now)
        .bind(&id)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if changed == 1 {
            insert_event(
                &mut transaction,
                &id,
                "outcome_unknown",
                "outcome_unknown",
                &actor,
                now,
                Some("Platform restarted before recording the helper result"),
            )
            .await?;
        }
        transaction.commit().await?;
    }
    reconcile_reboot_operations(&pool).await?;
    let monitor_pool = pool.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(15));
        interval.tick().await;
        loop {
            interval.tick().await;
            let _ = reconcile_reboot_operations(&monitor_pool).await;
        }
    });
    Ok(())
}

async fn reconcile_reboot_operations(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let awaiting_reboot = sqlx::query(
        "SELECT id,actor_profile_id,boot_id_before,started_at_unix
         FROM platform_server_operations WHERE state='awaiting_reboot'",
    )
    .fetch_all(pool)
    .await?;
    let current_boot_id = crate::server_operation_schema::current_boot_id();
    let now = unix_now();
    for row in awaiting_reboot {
        let id: String = row.get("id");
        let actor: String = row.get("actor_profile_id");
        let before: Option<String> = row.get("boot_id_before");
        let requested_at: Option<i64> = row.get("started_at_unix");
        let Some((to, event, detail)) = reboot_resolution(
            before.as_deref(),
            current_boot_id.as_deref(),
            requested_at,
            now,
        ) else {
            continue;
        };
        transition(
            pool,
            &id,
            OperationState::AwaitingReboot,
            to,
            &actor,
            event,
            now,
            Some(detail),
        )
        .await?;
    }
    Ok(())
}

async fn initialize_pool(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS platform_server_operations (
            id TEXT PRIMARY KEY,
            schema_version INTEGER NOT NULL CHECK(schema_version=1),
            client_request_id TEXT NOT NULL UNIQUE,
            target_node_id TEXT NOT NULL,
            action_json TEXT NOT NULL,
            state TEXT NOT NULL CHECK(state IN ('accepted','running','awaiting_reboot','succeeded','failed','outcome_unknown')),
            version INTEGER NOT NULL,
            actor_profile_id TEXT NOT NULL,
            created_at_unix INTEGER NOT NULL,
            started_at_unix INTEGER,
            finished_at_unix INTEGER,
            result_detail TEXT,
            boot_id_before TEXT
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_platform_server_operations_created
         ON platform_server_operations(created_at_unix DESC,id DESC)",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS platform_server_operation_events (
            event_id INTEGER PRIMARY KEY AUTOINCREMENT,
            operation_id TEXT NOT NULL REFERENCES platform_server_operations(id),
            event TEXT NOT NULL CHECK(event IN ('requested','started','reboot_requested','succeeded','failed','outcome_unknown')),
            state TEXT NOT NULL CHECK(state IN ('accepted','running','awaiting_reboot','succeeded','failed','outcome_unknown')),
            actor_profile_id TEXT NOT NULL,
            at_unix INTEGER NOT NULL,
            detail TEXT
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_platform_server_operation_events_id
         ON platform_server_operation_events(operation_id,event_id)",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TRIGGER IF NOT EXISTS platform_server_operation_events_no_update
         BEFORE UPDATE ON platform_server_operation_events
         BEGIN SELECT RAISE(ABORT, 'server operation events are append-only'); END",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TRIGGER IF NOT EXISTS platform_server_operation_events_no_delete
         BEFORE DELETE ON platform_server_operation_events
         BEGIN SELECT RAISE(ABORT, 'server operation events are append-only'); END",
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn create_operation(
    State(state): State<OperationsState>,
    headers: HeaderMap,
    Json(request): Json<CreateOperationRequest>,
) -> Response {
    let actor = match authorize_root(&state, &headers).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    if !same_origin(&headers) || !request.confirmed {
        return error(StatusCode::FORBIDDEN, "same_origin_confirmation_required");
    }
    if uuid::Uuid::parse_str(&request.client_request_id).is_err() {
        return error(StatusCode::BAD_REQUEST, "invalid_client_request_id");
    }
    let target = local_node_id();
    if request.target_node_id != target {
        return error(
            StatusCode::CONFLICT,
            "typed_executor_not_installed_for_server",
        );
    }
    let action = match ServerAction::parse_json(&request.action) {
        Ok(action) => action,
        Err(_) => return error(StatusCode::BAD_REQUEST, "unsupported_server_action"),
    };
    match &action {
        ServerAction::RestartService {
            service: ServiceId::Platform,
        } => {
            return error(
                StatusCode::NOT_IMPLEMENTED,
                "platform_self_restart_not_enabled",
            );
        }
        ServerAction::RestartService { .. } => {}
        ServerAction::RebootHost { confirmation }
            if confirmation == &format!("REBOOT {target}") => {}
        ServerAction::RebootHost { .. } => {
            return error(
                StatusCode::BAD_REQUEST,
                "reboot_confirmation_did_not_match_target",
            );
        }
    }
    let session_token = match crate::admin_auth::session_token(&headers) {
        Some(value) => value.to_owned(),
        None => return error(StatusCode::UNAUTHORIZED, "sign_in_required"),
    };
    let action_json = match serde_json::to_string(&action) {
        Ok(value) if value.len() <= 256 => value,
        _ => return error(StatusCode::BAD_REQUEST, "invalid_server_action"),
    };
    let id = uuid::Uuid::new_v4().to_string();
    let now = unix_now();
    let existing = match sqlx::query(
        "SELECT id,target_node_id,action_json,state,version,actor_profile_id,created_at_unix,started_at_unix,finished_at_unix,result_detail
         FROM platform_server_operations WHERE client_request_id=?",
    )
    .bind(&request.client_request_id)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(value) => value,
        Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE, "server_operation_state_unavailable"),
    };
    let (operation_id, operation_actor) = if let Some(row) = existing {
        if row.get::<String, _>("target_node_id") != target
            || row.get::<String, _>("action_json") != action_json
        {
            return error(
                StatusCode::CONFLICT,
                "client_request_id_reused_for_different_action",
            );
        }
        let existing_state: String = row.get("state");
        let existing_actor: String = row.get("actor_profile_id");
        let existing_id: String = row.get("id");
        if !accepted_operation_can_resume(&existing_state) {
            let record = operation_record(row);
            return private_json(
                StatusCode::OK,
                OperationDetail {
                    operation: record,
                    events: Vec::new(),
                },
            );
        }
        (existing_id, existing_actor)
    } else {
        let mut transaction = match state.pool.begin().await {
            Ok(value) => value,
            Err(_) => {
                return error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "server_operation_state_unavailable",
                );
            }
        };
        if sqlx::query(
            "INSERT INTO platform_server_operations
             (id,schema_version,client_request_id,target_node_id,action_json,state,version,actor_profile_id,created_at_unix)
             VALUES (?,1,?,?,?,'accepted',1,?,?)",
        )
        .bind(&id)
        .bind(&request.client_request_id)
        .bind(&target)
        .bind(&action_json)
        .bind(&actor)
        .bind(now)
        .execute(&mut *transaction)
        .await
        .is_err()
        {
            return error(StatusCode::CONFLICT, "server_operation_request_conflict");
        }
        if insert_event(
            &mut transaction,
            &id,
            "requested",
            "accepted",
            &actor,
            now,
            None,
        )
        .await
        .is_err()
        {
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "server_operation_state_unavailable",
            );
        }
        if transaction.commit().await.is_err() {
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "server_operation_state_unavailable",
            );
        }
        (id, actor.clone())
    };

    let running = transition(
        &state.pool,
        &operation_id,
        OperationState::Accepted,
        OperationState::Running,
        &operation_actor,
        "started",
        now,
        None,
    )
    .await;
    if !matches!(running, Ok(true)) {
        return operation_response(&state.pool, &operation_id, StatusCode::CONFLICT).await;
    }

    let payload = PlatformOperationRequest {
        operation: "server_action",
        request_id: &operation_id,
        expected_version: 2,
        session_token: &session_token,
        target_node_id: &target,
        action: &action,
    };
    let helper: Result<crate::platform_ops_client::OperationResult, &'static str> =
        crate::platform_ops_client::request(&payload).await;
    match helper {
        Ok(result)
            if result.operation_id == operation_id
                && result.ok
                && result.state == "succeeded"
                && bounded_detail(&result.detail) =>
        {
            let _ = transition(
                &state.pool,
                &operation_id,
                OperationState::Running,
                OperationState::Succeeded,
                &operation_actor,
                "succeeded",
                unix_now(),
                Some(&result.detail),
            )
            .await;
        }
        Ok(result)
            if result.operation_id == operation_id
                && result.ok
                && result.state == "awaiting_reboot"
                && bounded_detail(&result.detail) =>
        {
            // The helper persisted this state and the pre-reboot boot ID before
            // requesting the reboot. Startup reconciles it after the host returns.
        }
        Ok(result)
            if result.operation_id == operation_id
                && !result.ok
                && result.state == "failed"
                && bounded_detail(&result.detail) =>
        {
            let _ = transition(
                &state.pool,
                &operation_id,
                OperationState::Running,
                OperationState::Failed,
                &operation_actor,
                "failed",
                unix_now(),
                Some(&result.detail),
            )
            .await;
        }
        _ => {
            let _ = transition(
                &state.pool,
                &operation_id,
                OperationState::Running,
                OperationState::OutcomeUnknown,
                &operation_actor,
                "outcome_unknown",
                unix_now(),
                Some("The executor response was not conclusive; inspect the host before repeating this action."),
            )
            .await;
        }
    }
    operation_response(&state.pool, &operation_id, StatusCode::OK).await
}

async fn list_operations(State(state): State<OperationsState>, headers: HeaderMap) -> Response {
    if let Err(response) = authorize_root(&state, &headers).await {
        return response;
    }
    let rows = match sqlx::query(
        "SELECT id,target_node_id,action_json,state,version,actor_profile_id,created_at_unix,started_at_unix,finished_at_unix,result_detail
         FROM platform_server_operations ORDER BY created_at_unix DESC,id DESC LIMIT 100",
    )
    .fetch_all(&state.pool)
    .await
    {
        Ok(rows) => rows,
        Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE, "server_operation_state_unavailable"),
    };
    private_json(
        StatusCode::OK,
        OperationList {
            local_node_id: local_node_id(),
            items: rows.into_iter().map(operation_record).collect(),
        },
    )
}

async fn get_operation(
    State(state): State<OperationsState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err(response) = authorize_root(&state, &headers).await {
        return response;
    }
    if uuid::Uuid::parse_str(&id).is_err() {
        return error(StatusCode::BAD_REQUEST, "invalid_operation_id");
    }
    let row = match sqlx::query(
        "SELECT id,target_node_id,action_json,state,version,actor_profile_id,created_at_unix,started_at_unix,finished_at_unix,result_detail
         FROM platform_server_operations WHERE id=?",
    )
    .bind(&id)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(Some(row)) => row,
        Ok(None) => return error(StatusCode::NOT_FOUND, "server_operation_not_found"),
        Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE, "server_operation_state_unavailable"),
    };
    let events = match sqlx::query(
        "SELECT event,state,actor_profile_id,at_unix,detail FROM platform_server_operation_events
         WHERE operation_id=? ORDER BY event_id LIMIT 50",
    )
    .bind(&id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(rows) => rows
            .into_iter()
            .map(|row| OperationEvent {
                event: row.get("event"),
                state: row.get("state"),
                actor_profile_id: row.get("actor_profile_id"),
                at_unix: row.get("at_unix"),
                detail: row.get("detail"),
            })
            .collect(),
        Err(_) => {
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "server_operation_state_unavailable",
            );
        }
    };
    private_json(
        StatusCode::OK,
        OperationDetail {
            operation: operation_record(row),
            events,
        },
    )
}

async fn operation_response(pool: &SqlitePool, id: &str, status: StatusCode) -> Response {
    match fetch_operation(pool, id).await {
        Ok(Some(operation)) => {
            let events = sqlx::query(
                "SELECT event,state,actor_profile_id,at_unix,detail
                 FROM platform_server_operation_events WHERE operation_id=? ORDER BY event_id LIMIT 50",
            )
            .bind(id)
            .fetch_all(pool)
            .await;
            match events {
                Ok(rows) => private_json(
                    status,
                    OperationDetail {
                        operation: operation_record(operation),
                        events: rows
                            .into_iter()
                            .map(|row| OperationEvent {
                                event: row.get("event"),
                                state: row.get("state"),
                                actor_profile_id: row.get("actor_profile_id"),
                                at_unix: row.get("at_unix"),
                                detail: row.get("detail"),
                            })
                            .collect(),
                    },
                ),
                Err(_) => error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "server_operation_state_unavailable",
                ),
            }
        }
        _ => error(
            StatusCode::SERVICE_UNAVAILABLE,
            "server_operation_state_unavailable",
        ),
    }
}

async fn fetch_operation(
    pool: &SqlitePool,
    id: &str,
) -> Result<Option<sqlx::sqlite::SqliteRow>, sqlx::Error> {
    sqlx::query(
        "SELECT id,target_node_id,action_json,state,version,actor_profile_id,created_at_unix,started_at_unix,finished_at_unix,result_detail
         FROM platform_server_operations WHERE id=?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

fn operation_record(row: sqlx::sqlite::SqliteRow) -> OperationRecord {
    let action_json: String = row.get("action_json");
    let action = serde_json::from_str(&action_json).unwrap_or(serde_json::Value::Null);
    OperationRecord {
        id: row.get("id"),
        target_node_id: row.get("target_node_id"),
        action,
        state: row.get("state"),
        version: row.get("version"),
        actor_profile_id: row.get("actor_profile_id"),
        created_at_unix: row.get("created_at_unix"),
        started_at_unix: row.get("started_at_unix"),
        finished_at_unix: row.get("finished_at_unix"),
        result_detail: row.get("result_detail"),
    }
}

async fn transition(
    pool: &SqlitePool,
    id: &str,
    from: OperationState,
    to: OperationState,
    actor: &str,
    event: &str,
    now: i64,
    detail: Option<&str>,
) -> Result<bool, sqlx::Error> {
    if !allowed_transition(from, to) || detail.is_some_and(|value| !bounded_detail(value)) {
        return Ok(false);
    }
    let mut transaction = pool.begin().await?;
    let result = sqlx::query(
        "UPDATE platform_server_operations
         SET state=?,version=version+1,
             started_at_unix=CASE WHEN ?='running' THEN ? ELSE started_at_unix END,
             finished_at_unix=CASE WHEN ? IN ('succeeded','failed','outcome_unknown') THEN ? ELSE finished_at_unix END,
             result_detail=CASE WHEN ? IS NULL THEN result_detail ELSE ? END
         WHERE id=? AND state=?",
    )
    .bind(to.as_str())
    .bind(to.as_str())
    .bind(now)
    .bind(to.as_str())
    .bind(now)
    .bind(detail)
    .bind(detail)
    .bind(id)
    .bind(from.as_str())
    .execute(&mut *transaction)
    .await?;
    if result.rows_affected() == 1 {
        insert_event(&mut transaction, id, event, to.as_str(), actor, now, detail).await?;
    }
    transaction.commit().await?;
    Ok(result.rows_affected() == 1)
}

async fn insert_event(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    id: &str,
    event: &str,
    state: &str,
    actor: &str,
    now: i64,
    detail: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO platform_server_operation_events
         (operation_id,event,state,actor_profile_id,at_unix,detail) VALUES (?,?,?,?,?,?)",
    )
    .bind(id)
    .bind(event)
    .bind(state)
    .bind(actor)
    .bind(now)
    .bind(detail)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn bounded_detail(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_RESULT_DETAIL_BYTES
        && !value.chars().any(char::is_control)
}

async fn authorize_root(state: &OperationsState, headers: &HeaderMap) -> Result<String, Response> {
    match crate::admin_auth::authorize_root_profile_with_client(
        headers,
        state.service_token.as_deref(),
        &state.authz_endpoint,
        &state.client,
    )
    .await
    {
        Ok(profile_id) => Ok(profile_id),
        Err(failure) => Err(error(
            failure.status(),
            match failure {
                crate::admin_auth::AuthFailure::Unauthorized => "sign_in_required",
                crate::admin_auth::AuthFailure::Forbidden => "root_required",
                crate::admin_auth::AuthFailure::Unavailable => "identity_authorization_unavailable",
            },
        )),
    }
}

fn same_origin(headers: &HeaderMap) -> bool {
    headers
        .get(axum::http::header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|origin| origin == crate::admin_auth::browser_admin_origin())
}

async fn no_store(request: axum::http::Request<Body>, next: axum::middleware::Next) -> Response {
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        CACHE_CONTROL,
        axum::http::HeaderValue::from_static("private, no-store"),
    );
    response
}

fn private_json<T: Serialize>(status: StatusCode, value: T) -> Response {
    let mut response = (status, Json(value)).into_response();
    response.headers_mut().insert(
        CACHE_CONTROL,
        axum::http::HeaderValue::from_static("private, no-store"),
    );
    response
}

fn error(status: StatusCode, code: &'static str) -> Response {
    private_json(status, serde_json::json!({"error":code}))
}

fn state() -> OperationsState {
    OperationsState {
        pool: create_pool(),
        authz_endpoint: std::env::var("VAPOR_PLATFORM_IDENTITY_AUTHZ_URL")
            .unwrap_or_else(|_| DEFAULT_IDENTITY_AUTHZ_URL.to_owned()),
        service_token: std::env::var("VAPOR_PLATFORM_IDENTITY_AUTHZ_TOKEN").ok(),
        client: reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(1))
            .timeout(Duration::from_secs(3))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("valid server-operation authorization client"),
    }
}

fn create_pool() -> SqlitePool {
    let path = std::env::var_os("VAPOR_PLATFORM_DB")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("VAPOR_PLATFORM_STATE")
                .map(PathBuf::from)
                .map(|directory| directory.join("platform.sqlite3"))
        })
        .unwrap_or_else(|| PathBuf::from("/var/lib/vapor-server/platform/platform.sqlite3"));
    SqlitePoolOptions::new()
        .max_connections(5)
        .connect_lazy_with(
            SqliteConnectOptions::new()
                .filename(path)
                .create_if_missing(true)
                .foreign_keys(true)
                .busy_timeout(Duration::from_secs(5))
                .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal),
        )
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{
        allowed_transition, initialize_pool, router_with_state, OperationState, OperationsState,
    };
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use sqlx::sqlite::SqlitePoolOptions;
    use tower::ServiceExt;

    #[test]
    fn operation_state_transitions_do_not_allow_implicit_retries() {
        use OperationState::*;
        assert!(allowed_transition(Accepted, Running));
        assert!(allowed_transition(Running, Succeeded));
        assert!(allowed_transition(Running, Failed));
        assert!(allowed_transition(Running, OutcomeUnknown));
        assert!(allowed_transition(OutcomeUnknown, Succeeded));
        assert!(!allowed_transition(OutcomeUnknown, Running));
        assert!(!allowed_transition(Succeeded, Running));
        assert!(!allowed_transition(Failed, Running));
        assert!(super::accepted_operation_can_resume("accepted"));
        assert!(!super::accepted_operation_can_resume("running"));
        assert!(!super::accepted_operation_can_resume("outcome_unknown"));
        assert!(!super::accepted_operation_can_resume("succeeded"));
        assert_eq!(
            super::reboot_resolution(Some("boot-a"), Some("boot-b"), Some(10), 20)
                .map(|(state, _, _)| state),
            Some(Succeeded)
        );
        assert_eq!(
            super::reboot_resolution(Some("boot-a"), Some("boot-a"), Some(10), 310)
                .map(|(state, _, _)| state),
            Some(OutcomeUnknown)
        );
        assert!(super::reboot_resolution(Some("boot-a"), Some("boot-a"), Some(10), 20).is_none());
    }

    #[tokio::test]
    async fn operation_schema_initializes_with_append_only_events() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        initialize_pool(&pool).await.unwrap();
        let tables: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('platform_server_operations','platform_server_operation_events')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(tables, 2);
        let triggers: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='trigger' AND name IN ('platform_server_operation_events_no_update','platform_server_operation_events_no_delete')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(triggers, 2);
        sqlx::query(
            "INSERT INTO platform_server_operations
             (id,schema_version,client_request_id,target_node_id,action_json,state,version,actor_profile_id,created_at_unix)
             VALUES ('op-test',1,'request-test','node-test','{}','accepted',1,'actor-test',100)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO platform_server_operation_events
             (operation_id,event,state,actor_profile_id,at_unix) VALUES ('op-test','requested','accepted','actor-test',100)",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            sqlx::query("UPDATE platform_server_operation_events SET detail='edited'")
                .execute(&pool)
                .await
                .is_err()
        );
        assert!(sqlx::query("DELETE FROM platform_server_operation_events")
            .execute(&pool)
            .await
            .is_err());
        let event_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM platform_server_operation_events")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(event_count, 1);
    }

    #[tokio::test]
    async fn operation_endpoints_require_a_root_session_before_reading_state() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        initialize_pool(&pool).await.unwrap();
        let app = router_with_state(OperationsState {
            pool,
            authz_endpoint: "http://127.0.0.1:9/authz".to_owned(),
            service_token: None,
            client: reqwest::Client::new(),
        });
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/admin/server-operations")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
