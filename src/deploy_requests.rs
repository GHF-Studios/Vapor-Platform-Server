use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::header::CACHE_CONTROL;
use axum::http::{HeaderMap, HeaderValue, Request, StatusCode};
use axum::middleware::{from_fn, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::path::PathBuf;
use std::time::Duration;

const REQUEST_TTL_SECONDS: i64 = 3 * 24 * 60 * 60;
const MAX_REQUESTS: i64 = 200;
const DISPATCH_STALE_SECONDS: i64 = 30;
const SOURCE_REPOSITORY: &str = "GHF-Studios/Vapor-Platform-Server";
const DEPLOY_BRANCH: &str = "main";
const SUBMODULES: &[&str] = &[
    "Atlas",
    "Vapor-Artifact-Server",
    "Vapor-Build-Cache-Server",
    "Vapor-Diagnostics-Server",
    "Vapor-Docs-Server",
    "Vapor-Homepage-Server",
    "Vapor-Identity-Server",
    "Vapor-Registry-Server",
];

#[derive(Clone)]
struct DeployRequestState {
    pool: SqlitePool,
    client: reqwest::Client,
    identity_root_authz_url: String,
    identity_root_authz_token: Option<String>,
    public_origin: String,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
struct SubmodulePin {
    path: String,
    commit: String,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
struct CreateDeployRequest {
    server_mode: String,
    deployment_scope: String,
    service_target: Option<String>,
    repository: String,
    branch: String,
    commit: String,
    tree: String,
    submodules: Vec<SubmodulePin>,
    workflow_url: String,
    summary: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DecideRequest {
    expected_version: i64,
    confirmed: bool,
    reason: Option<String>,
}

#[derive(Serialize)]
struct DeployRequestSummary {
    id: String,
    requester_profile_id: String,
    requester_kind: String,
    repository: String,
    branch: String,
    commit: String,
    tree: String,
    submodules: Vec<SubmodulePin>,
    server_mode: Option<String>,
    deployment_scope: Option<String>,
    service_target: Option<String>,
    workflow_url: String,
    summary: String,
    status: String,
    version: i64,
    created_at_unix: i64,
    expires_at_unix: i64,
    decided_at_unix: Option<i64>,
    decision_reason: Option<String>,
    result_detail: Option<String>,
    dispatch_state: String,
    dispatch_attempts: i64,
    dispatch_attempted_at_unix: Option<i64>,
    dispatch_detail: Option<String>,
    retry_eligible: bool,
}

#[derive(Serialize)]
struct CreatedRequest {
    id: String,
    status: String,
    version: i64,
    expires_at_unix: i64,
}

struct ActiveRequest {
    created: CreatedRequest,
    request_json: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct IdentityCredentialDecision {
    pub(crate) credential_id: String,
    pub(crate) actor_profile_id: String,
    pub(crate) scope: String,
    #[serde(default)]
    pub(crate) bound_ssh_fingerprint: Option<String>,
    #[serde(default)]
    pub(crate) bound_ssh_request_hash: Option<String>,
}

const MAX_IDENTITY_CREDENTIAL_RESPONSE_BYTES: usize = 4096;

pub(crate) fn router() -> Router {
    let state = state();
    Router::new()
        .route(
            "/api/admin/deploy-requests",
            get(list_requests).post(create_request),
        )
        .route(
            "/api/admin/deploy-requests/{id}/approve",
            post(approve_request),
        )
        .route(
            "/api/admin/deploy-requests/{id}/reject",
            post(reject_request),
        )
        .route(
            "/api/admin/deploy-requests/{id}/dispatch",
            post(retry_dispatch),
        )
        .route(
            "/api/admin/deploy-requests/{id}/reconcile",
            post(reconcile_execution),
        )
        .with_state(state)
        .route_layer(from_fn(no_store))
}

pub(crate) async fn initialize() -> Result<(), Box<dyn std::error::Error>> {
    let state_dir = state_directory();
    tokio::fs::create_dir_all(&state_dir).await?;
    let pool = create_pool();
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS platform_deploy_requests (
            id TEXT PRIMARY KEY,
            body_hash TEXT NOT NULL CHECK(length(body_hash) = 64),
            request_json TEXT NOT NULL,
            repository TEXT NOT NULL,
            branch TEXT NOT NULL,
            commit_hash TEXT NOT NULL CHECK(length(commit_hash) = 40),
            tree_hash TEXT NOT NULL CHECK(length(tree_hash) = 40),
            submodules_json TEXT NOT NULL,
            workflow_url TEXT NOT NULL,
            summary TEXT NOT NULL,
            requester_credential_id TEXT NOT NULL,
            requester_profile_id TEXT NOT NULL,
            status TEXT NOT NULL CHECK(status IN ('pending','approved','executing','succeeded','failed','rejected','expired')),
            version INTEGER NOT NULL DEFAULT 1 CHECK(version > 0),
            created_at_unix INTEGER NOT NULL,
            expires_at_unix INTEGER NOT NULL,
            decided_by_profile_id TEXT,
            decided_at_unix INTEGER,
            decision_reason TEXT,
            result_detail TEXT
        )",
    )
    .execute(&pool)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_platform_deploy_requests_state ON platform_deploy_requests(status, expires_at_unix, created_at_unix)")
        .execute(&pool).await?;
    sqlx::query("CREATE UNIQUE INDEX IF NOT EXISTS idx_platform_deploy_requests_one_active_commit ON platform_deploy_requests(commit_hash) WHERE status IN ('pending','approved','executing')")
        .execute(&pool).await?;
    crate::deploy_request_schema::ensure_dispatch_columns(&pool).await?;
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

fn state() -> DeployRequestState {
    let domain =
        std::env::var("VAPOR_DOMAIN").unwrap_or_else(|_| "vapor.ghf-studios.site".to_string());
    DeployRequestState {
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
    }
}

async fn create_request(
    State(state): State<DeployRequestState>,
    headers: HeaderMap,
    Json(request): Json<CreateDeployRequest>,
) -> Response {
    let Some(token) = bearer_token(&headers) else {
        return (StatusCode::UNAUTHORIZED, "request credential required\n").into_response();
    };
    let credential = match verify_request_credential(token).await {
        Ok(credential) => credential,
        Err(response) => return response,
    };
    if let Err(message) = validate_request(&request) {
        return (StatusCode::BAD_REQUEST, message).into_response();
    }
    if expire_requests(&state.pool, unix_now()).await.is_err() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let bytes = match serde_json::to_vec(&request) {
        Ok(bytes) => bytes,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    let body_hash = sha256_hex(&bytes);
    let request_json = String::from_utf8(bytes).unwrap_or_default();
    let now = unix_now();
    let expires_at = now + REQUEST_TTL_SECONDS;
    let submodules = serde_json::to_string(&request.submodules).unwrap_or_default();
    match active_request_for_commit(&state.pool, &request.commit).await {
        Ok(Some(existing)) => return existing_request_response(existing, &request),
        Ok(None) => {}
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
    let id = uuid::Uuid::new_v4().to_string();
    let result = sqlx::query(
        "INSERT INTO platform_deploy_requests
         (id, body_hash, request_json, repository, branch, commit_hash, tree_hash, submodules_json,
          workflow_url, summary, requester_credential_id, requester_profile_id, status, version, created_at_unix, expires_at_unix)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending', 1, ?, ?)",
    )
    .bind(&id).bind(body_hash).bind(request_json).bind(&request.repository).bind(&request.branch)
    .bind(&request.commit).bind(&request.tree).bind(submodules).bind(&request.workflow_url)
    .bind(&request.summary).bind(credential.credential_id).bind(credential.actor_profile_id).bind(now).bind(expires_at)
    .execute(&state.pool).await;
    if result.is_err() {
        match active_request_for_commit(&state.pool, &request.commit).await {
            Ok(Some(existing)) => return existing_request_response(existing, &request),
            Ok(None) => {}
            Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
        }
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    (
        StatusCode::ACCEPTED,
        Json(CreatedRequest {
            id,
            status: "pending".to_owned(),
            version: 1,
            expires_at_unix: expires_at,
        }),
    )
        .into_response()
}

async fn active_request_for_commit(
    pool: &SqlitePool,
    commit: &str,
) -> Result<Option<ActiveRequest>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, status, version, expires_at_unix, request_json FROM platform_deploy_requests
         WHERE commit_hash = ? AND status IN ('pending','approved','executing') LIMIT 1",
    )
    .bind(commit)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|row| ActiveRequest {
        created: CreatedRequest {
            id: row.get("id"),
            status: row.get("status"),
            version: row.get("version"),
            expires_at_unix: row.get("expires_at_unix"),
        },
        request_json: row.get("request_json"),
    }))
}

fn existing_request_response(existing: ActiveRequest, incoming: &CreateDeployRequest) -> Response {
    if same_request_shape(&existing.request_json, incoming) {
        (StatusCode::ACCEPTED, Json(existing.created)).into_response()
    } else {
        (
            StatusCode::CONFLICT,
            "another active deployment request already exists for this root commit with a different topology, scope, or target; wait for it to finish before submitting another\n",
        )
            .into_response()
    }
}

fn same_request_shape(stored_json: &str, incoming: &CreateDeployRequest) -> bool {
    let Ok(stored) = serde_json::from_str::<CreateDeployRequest>(stored_json) else {
        return false;
    };
    stored.commit == incoming.commit
        && stored.server_mode == incoming.server_mode
        && stored.deployment_scope == incoming.deployment_scope
        && stored.service_target == incoming.service_target
}

async fn list_requests(State(state): State<DeployRequestState>, headers: HeaderMap) -> Response {
    let owner = match require_root(&state, &headers).await {
        Ok(_) => None,
        Err(root_failure) => {
            let Some(token) = bearer_token(&headers) else {
                return root_failure;
            };
            match verify_request_credential(token).await {
                Ok(credential) => Some(credential.credential_id),
                Err(_) => return root_failure,
            }
        }
    };
    if expire_requests(&state.pool, unix_now()).await.is_err() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    if normalize_stale_dispatches(&state.pool, unix_now())
        .await
        .is_err()
    {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let rows = match fetch_request_rows(&state.pool, owner.as_deref()).await {
        Ok(rows) => rows,
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let requests = rows
        .into_iter()
        .filter_map(|row| {
            let submodules: Vec<SubmodulePin> =
                serde_json::from_str(row.get("submodules_json")).ok()?;
            Some(DeployRequestSummary {
                id: row.get("id"),
                requester_profile_id: row.get("requester_profile_id"),
                requester_kind: if row
                    .get::<Option<String>, _>("requester_credential_id")
                    .is_some()
                {
                    "request credential".to_owned()
                } else {
                    "interactive session".to_owned()
                },
                repository: row.get("repository"),
                branch: row.get("branch"),
                commit: row.get("commit_hash"),
                tree: row.get("tree_hash"),
                submodules,
                server_mode: request_server_mode(row.get("request_json")),
                deployment_scope: request_scope(row.get("request_json")),
                service_target: request_service_target(row.get("request_json")),
                workflow_url: row.get("workflow_url"),
                summary: row.get("summary"),
                status: row.get("status"),
                version: row.get("version"),
                created_at_unix: row.get("created_at_unix"),
                expires_at_unix: row.get("expires_at_unix"),
                decided_at_unix: row.get("decided_at_unix"),
                decision_reason: row.get("decision_reason"),
                result_detail: row.get("result_detail"),
                dispatch_state: row.get("dispatch_state"),
                dispatch_attempts: row.get("dispatch_attempts"),
                dispatch_attempted_at_unix: row.get("dispatch_attempted_at_unix"),
                dispatch_detail: row.get("dispatch_detail"),
                retry_eligible: retry_eligible(
                    row.get("status"),
                    row.get("dispatch_state"),
                    row.get("dispatch_attempted_at_unix"),
                    unix_now(),
                ),
            })
        })
        .collect::<Vec<_>>();
    Json(requests).into_response()
}

async fn fetch_request_rows(
    pool: &SqlitePool,
    owner_credential_id: Option<&str>,
) -> Result<Vec<sqlx::sqlite::SqliteRow>, sqlx::Error> {
    let query = "SELECT id, requester_profile_id, requester_credential_id, repository, branch, commit_hash, tree_hash, submodules_json, request_json, workflow_url,
                        summary, status, version, created_at_unix, expires_at_unix, decided_at_unix, decision_reason, result_detail,
                        dispatch_state, dispatch_attempts, dispatch_attempted_at_unix, dispatch_detail
                 FROM platform_deploy_requests";
    if let Some(owner) = owner_credential_id {
        sqlx::query(&format!(
            "{query} WHERE requester_credential_id = ? ORDER BY created_at_unix DESC LIMIT ?"
        ))
        .bind(owner)
        .bind(MAX_REQUESTS)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query(&format!("{query} ORDER BY created_at_unix DESC LIMIT ?"))
            .bind(MAX_REQUESTS)
            .fetch_all(pool)
            .await
    }
}

async fn expire_requests(pool: &SqlitePool, now: i64) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE platform_deploy_requests SET status = 'expired', version = version + 1,
         decided_at_unix = ?,
         dispatch_state = CASE
             WHEN status = 'approved' AND dispatch_state IN ('queued','dispatching') THEN 'expired'
             ELSE dispatch_state
         END,
         dispatch_detail = CASE
             WHEN status = 'approved' AND dispatch_state IN ('queued','dispatching')
             THEN 'Approval expired before the worker claimed this request.'
             ELSE dispatch_detail
         END
         WHERE status IN ('pending','approved') AND expires_at_unix <= ?",
    )
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

async fn approve_request(
    State(state): State<DeployRequestState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<DecideRequest>,
) -> Response {
    decide_request(state, headers, id, request, true).await
}

async fn reject_request(
    State(state): State<DeployRequestState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<DecideRequest>,
) -> Response {
    decide_request(state, headers, id, request, false).await
}

async fn decide_request(
    state: DeployRequestState,
    headers: HeaderMap,
    id: String,
    request: DecideRequest,
    approve: bool,
) -> Response {
    if request.expected_version < 1 || !request.confirmed {
        return (
            StatusCode::BAD_REQUEST,
            "positive request version and explicit confirmation required\n",
        )
            .into_response();
    }
    if !same_origin(&state, &headers) {
        return (StatusCode::FORBIDDEN, "same-origin request required\n").into_response();
    }
    let actor_profile_id = match require_root(&state, &headers).await {
        Ok(actor) => actor,
        Err(response) if response.status() == StatusCode::SERVICE_UNAVAILABLE => {
            return stage_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "authorization_unavailable",
                "authorization",
                "The approval was not saved because root authorization could not be checked. Retry after Identity is available.",
                false,
                false,
            );
        }
        Err(response) => return response,
    };
    let Some(session_token) = crate::admin_auth::session_token(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let reason = request
        .reason
        .as_deref()
        .unwrap_or(if approve {
            "approved by root operator"
        } else {
            "rejected by root operator"
        })
        .trim();
    if reason.is_empty() || reason.len() > 500 || reason.chars().any(char::is_control) {
        return (StatusCode::BAD_REQUEST, "invalid decision reason\n").into_response();
    }
    if approve {
        let stored_json = match sqlx::query_scalar::<_, String>(
            "SELECT request_json FROM platform_deploy_requests WHERE id = ? AND status = 'pending' AND version = ?",
        )
        .bind(&id)
        .bind(request.expected_version)
        .fetch_optional(&state.pool)
        .await
        {
            Ok(Some(value)) => value,
            Ok(None) => return (StatusCode::CONFLICT, "request is expired or no longer pending at that version\n").into_response(),
            Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
        };
        if request_server_mode(&stored_json).is_none() {
            return (
                StatusCode::CONFLICT,
                "this legacy request has no valid deployment target; reject it and submit a fresh CI request\n",
            )
                .into_response();
        }
    }
    let now = unix_now();
    let next = if approve { "approved" } else { "rejected" };
    let result = sqlx::query(
        "UPDATE platform_deploy_requests SET status = ?, version = version + 1,
         decided_by_profile_id = ?, decided_at_unix = ?, decision_reason = ?
         WHERE id = ? AND status = 'pending' AND version = ? AND expires_at_unix > ?",
    )
    .bind(next)
    .bind(actor_profile_id)
    .bind(now)
    .bind(reason)
    .bind(&id)
    .bind(request.expected_version)
    .bind(now)
    .execute(&state.pool)
    .await;
    match result {
        Ok(result) if result.rows_affected() == 1 && approve => {
            dispatch_approved_authorized(
                &state,
                &id,
                request.expected_version + 1,
                session_token,
            )
            .await
        }
        Ok(result) if result.rows_affected() == 1 => (StatusCode::OK, Json(serde_json::json!({"id": id, "status": next, "version": request.expected_version + 1}))).into_response(),
        Ok(_) => (StatusCode::CONFLICT, "request is expired or no longer pending at that version\n").into_response(),
        Err(error) => {
            eprintln!("deploy request {id}: stage=decision_persistence category=database_update_failed error={error}");
            stage_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "decision_not_saved",
                "decision",
                if approve {
                    "Approval was not saved. The request remains available for review."
                } else {
                    "Rejection was not saved. The request remains available for review."
                },
                false,
                false,
            )
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DispatchRequest {
    expected_version: i64,
    confirmed: bool,
}

async fn retry_dispatch(
    State(state): State<DeployRequestState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<DispatchRequest>,
) -> Response {
    if !request.confirmed || request.expected_version < 1 || !valid_uuid(&id) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if !same_origin(&state, &headers) {
        return (StatusCode::FORBIDDEN, "same-origin request required\n").into_response();
    }
    if let Err(response) = require_root(&state, &headers).await {
        return response;
    }
    let Some(session_token) = crate::admin_auth::session_token(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    dispatch_approved_authorized(&state, &id, request.expected_version, session_token).await
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct ReconcileOperation<'a> {
    operation: &'static str,
    request_id: &'a str,
    expected_version: i64,
    session_token: &'a str,
}

async fn reconcile_execution(
    State(state): State<DeployRequestState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<DispatchRequest>,
) -> Response {
    if !request.confirmed || request.expected_version < 1 || !valid_uuid(&id) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if !same_origin(&state, &headers) {
        return (StatusCode::FORBIDDEN, "same-origin request required\n").into_response();
    }
    if let Err(response) = require_root(&state, &headers).await {
        return response;
    }
    let Some(session_token) = crate::admin_auth::session_token(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let row = match sqlx::query("SELECT status, version FROM platform_deploy_requests WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.pool)
        .await
    {
        Ok(Some(row)) => row,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    if row.get::<String, _>("status") != "executing"
        || row.get::<i64, _>("version") != request.expected_version
    {
        return StatusCode::CONFLICT.into_response();
    }
    let operation = ReconcileOperation {
        operation: "reconcile_deploy",
        request_id: &id,
        expected_version: request.expected_version,
        session_token,
    };
    match invoke_platform_ops(&operation, &id, "failed").await {
        Ok(output) => (StatusCode::OK, Json(output)).into_response(),
        Err(_) => (
            StatusCode::CONFLICT,
            "worker is active, state changed, or reconciliation failed\n",
        )
            .into_response(),
    }
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct DispatchOperation<'a> {
    operation: &'static str,
    request_id: &'a str,
    expected_version: i64,
    session_token: &'a str,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PlatformOperationResult {
    ok: bool,
    operation_id: String,
    state: String,
    detail: String,
}

async fn dispatch_approved_authorized(
    state: &DeployRequestState,
    id: &str,
    version: i64,
    session_token: &str,
) -> Response {
    if !valid_uuid(id) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let stored_json = match sqlx::query_scalar::<_, String>(
        "SELECT request_json FROM platform_deploy_requests WHERE id = ? AND status = 'approved' AND version = ?",
    )
    .bind(id)
    .bind(version)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(Some(value)) => value,
        Ok(None) => return StatusCode::CONFLICT.into_response(),
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    if request_server_mode(&stored_json).is_none() {
        return stage_error(
            StatusCode::CONFLICT,
            "dispatch_not_started",
            "dispatch",
            "This legacy request has no valid deployment target. Submit a fresh CI request; no deployment was queued.",
            true,
            false,
        );
    }
    if expire_requests(&state.pool, unix_now()).await.is_err() {
        return stage_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "dispatch_not_started",
            "request_state",
            "Approval is saved, but the current request state could not be checked. Reload before retrying.",
            true,
            false,
        );
    }
    let attempt = match begin_dispatch_attempt(&state.pool, id, version, unix_now()).await {
        Ok(Some(attempt)) => attempt,
        Ok(None) => {
            return stage_error(
                StatusCode::CONFLICT,
                "dispatch_not_started",
                "dispatch",
                "This revision is no longer approved for dispatch, or a fresh dispatch attempt is already running.",
                true,
                false,
            );
        }
        Err(error) => {
            eprintln!(
                "deploy request {id}: stage=dispatch_persistence category=database_update_failed error={error}"
            );
            return stage_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "dispatch_not_started",
                "dispatch",
                "Approval is saved, but the dispatch attempt could not be recorded.",
                true,
                false,
            );
        }
    };
    let operation = DispatchOperation {
        operation: "dispatch_deploy",
        request_id: id,
        expected_version: version,
        session_token,
    };
    let mut dispatch_failure = None;
    let (dispatch_state, detail, output) = match invoke_platform_ops(&operation, id, "queued").await
    {
        Ok(value) => ("queued", "Approved deployment queued.", Some(value)),
        Err(error) => {
            eprintln!(
                "deploy request {id}: stage=dispatch category={} error={}",
                error.category, error.technical
            );
            let state = error.state;
            let detail = error.public_message;
            dispatch_failure = Some(error);
            (state, detail, None)
        }
    };
    let recorded = match finish_dispatch_attempt(&state.pool, id, attempt, dispatch_state, detail)
        .await
    {
        Ok(recorded) => recorded,
        Err(error) => {
            eprintln!(
                "deploy request {id}: stage=dispatch_persistence category=database_update_failed error={error}"
            );
            return stage_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "dispatch_result_not_recorded",
                "dispatch_recording",
                "Approval is saved, but the dispatch result could not be recorded. Reload the request before retrying.",
                true,
                false,
            );
        }
    };
    if !recorded {
        return stage_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "dispatch_result_not_recorded",
            "dispatch_recording",
            "The request changed while dispatch was running. Reload it to see the current deployment state.",
            true,
            false,
        );
    }
    if dispatch_state == "queued" {
        (
            StatusCode::OK,
            Json(serde_json::json!({"id":id,"status":"approved","version":version,"decision":{"status":"approved","saved":true},"dispatch":{"status":"queued","detail":detail,"result":output}})),
        ).into_response()
    } else {
        let retry_allowed = dispatch_retry_allowed(&state.pool, id, version, unix_now())
            .await
            .unwrap_or(false);
        if let Some(error) = dispatch_failure.as_ref() {
            dispatch_failure_response(error, retry_allowed)
        } else {
            stage_error(
                StatusCode::SERVICE_UNAVAILABLE,
                if dispatch_state == "unknown" {
                    "dispatch_result_unknown"
                } else {
                    "dispatch_failed"
                },
                "dispatch",
                detail,
                true,
                retry_allowed,
            )
        }
    }
}

async fn dispatch_retry_allowed(
    pool: &SqlitePool,
    id: &str,
    version: i64,
    now: i64,
) -> Result<bool, sqlx::Error> {
    let row = sqlx::query(
        "SELECT status, dispatch_state, dispatch_attempted_at_unix
         FROM platform_deploy_requests WHERE id = ? AND version = ?",
    )
    .bind(id)
    .bind(version)
    .fetch_optional(pool)
    .await?;
    Ok(row.is_some_and(|row| {
        retry_eligible(
            row.get("status"),
            row.get("dispatch_state"),
            row.get("dispatch_attempted_at_unix"),
            now,
        )
    }))
}

#[derive(Debug)]
struct DispatchFailure {
    state: &'static str,
    category: &'static str,
    technical: &'static str,
    public_message: &'static str,
}

fn dispatch_failure_response(error: &DispatchFailure, retry_allowed: bool) -> Response {
    stage_error(
        StatusCode::SERVICE_UNAVAILABLE,
        if error.category == "helper_protocol_mismatch" {
            "helper_protocol_mismatch"
        } else if error.state == "unknown" {
            "dispatch_result_unknown"
        } else {
            "dispatch_failed"
        },
        "dispatch",
        error.public_message,
        true,
        retry_allowed,
    )
}

fn stage_error(
    status: StatusCode,
    code: &'static str,
    stage: &'static str,
    message: &'static str,
    decision_saved: bool,
    retry_eligible: bool,
) -> Response {
    (
        status,
        Json(serde_json::json!({
            "error": {
                "code": code,
                "stage": stage,
                "message": message,
                "decision_saved": decision_saved,
                "retry_eligible": retry_eligible
            }
        })),
    )
        .into_response()
}

fn retry_eligible(
    status: String,
    dispatch_state: String,
    attempted_at: Option<i64>,
    now: i64,
) -> bool {
    status == "approved"
        && match dispatch_state.as_str() {
            "not_attempted" | "failed" | "unknown" => true,
            "dispatching" => attempted_at
                .is_some_and(|attempted_at| attempted_at <= now - DISPATCH_STALE_SECONDS),
            _ => false,
        }
}

async fn begin_dispatch_attempt(
    pool: &SqlitePool,
    id: &str,
    approved_version: i64,
    now: i64,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar::<_, i64>(
        "UPDATE platform_deploy_requests
         SET dispatch_state = 'dispatching', dispatch_attempts = dispatch_attempts + 1,
             dispatch_attempted_at_unix = ?, dispatch_detail = NULL
         WHERE id = ? AND status = 'approved' AND version = ?
           AND (dispatch_state IN ('not_attempted','failed','unknown')
                OR (dispatch_state = 'dispatching' AND dispatch_attempted_at_unix <= ?))
         RETURNING dispatch_attempts",
    )
    .bind(now)
    .bind(id)
    .bind(approved_version)
    .bind(now - DISPATCH_STALE_SECONDS)
    .fetch_optional(pool)
    .await
}

async fn normalize_stale_dispatches(pool: &SqlitePool, now: i64) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE platform_deploy_requests
         SET dispatch_state = 'unknown',
             dispatch_detail = 'Previous dispatch attempt stopped responding; retry is available.'
         WHERE status = 'approved' AND dispatch_state = 'dispatching'
           AND dispatch_attempted_at_unix <= ?",
    )
    .bind(now - DISPATCH_STALE_SECONDS)
    .execute(pool)
    .await?;
    Ok(())
}

async fn finish_dispatch_attempt(
    pool: &SqlitePool,
    id: &str,
    attempt: i64,
    outcome: &str,
    detail: &str,
) -> Result<bool, sqlx::Error> {
    let bounded_detail = detail.chars().take(240).collect::<String>();
    let result = sqlx::query(
        "UPDATE platform_deploy_requests
         SET dispatch_state = ?, dispatch_detail = ?
         WHERE id = ? AND dispatch_attempts = ? AND dispatch_state = 'dispatching'
           AND status != 'expired'",
    )
    .bind(outcome)
    .bind(bounded_detail)
    .bind(id)
    .bind(attempt)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

#[cfg(test)]
mod dispatch_tests {
    use super::{
        begin_dispatch_attempt, expire_requests, finish_dispatch_attempt,
        normalize_stale_dispatches, retry_eligible,
    };
    use sqlx::sqlite::SqlitePoolOptions;
    use sqlx::Row;

    async fn pool_with_approved_request() -> sqlx::SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("CREATE TABLE platform_deploy_requests (id TEXT PRIMARY KEY, status TEXT NOT NULL, version INTEGER NOT NULL, dispatch_state TEXT NOT NULL DEFAULT 'not_attempted', dispatch_attempts INTEGER NOT NULL DEFAULT 0, dispatch_attempted_at_unix INTEGER, dispatch_detail TEXT)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO platform_deploy_requests (id, status, version) VALUES ('r1', 'approved', 3)")
            .execute(&pool).await.unwrap();
        pool
    }

    async fn pool_with_expiring_requests() -> sqlx::SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("CREATE TABLE platform_deploy_requests (id TEXT PRIMARY KEY, status TEXT NOT NULL, version INTEGER NOT NULL, expires_at_unix INTEGER NOT NULL, decided_at_unix INTEGER, dispatch_state TEXT NOT NULL DEFAULT 'not_attempted', dispatch_attempts INTEGER NOT NULL DEFAULT 0, dispatch_attempted_at_unix INTEGER, dispatch_detail TEXT)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO platform_deploy_requests (id, status, version, expires_at_unix, dispatch_state, dispatch_attempts, dispatch_attempted_at_unix) VALUES ('queued', 'approved', 3, 99, 'queued', 1, 90), ('dispatching', 'approved', 3, 99, 'dispatching', 1, 90), ('pending', 'pending', 1, 99, 'not_attempted', 0, NULL), ('future', 'approved', 2, 101, 'queued', 1, 90), ('executing', 'executing', 4, 99, 'queued', 1, 90)")
            .execute(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn expired_queued_deploy_cannot_keep_a_false_queued_state_or_accept_late_result() {
        let pool = pool_with_expiring_requests().await;

        expire_requests(&pool, 100).await.unwrap();
        let queued = sqlx::query("SELECT status, dispatch_state, dispatch_detail FROM platform_deploy_requests WHERE id='queued'")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(queued.get::<String, _>("status"), "expired");
        assert_eq!(queued.get::<String, _>("dispatch_state"), "expired");
        assert!(queued
            .get::<String, _>("dispatch_detail")
            .contains("expired before the worker claimed"));
        assert!(!retry_eligible(
            queued.get("status"),
            queued.get("dispatch_state"),
            Some(90),
            100
        ));

        let dispatching = sqlx::query(
            "SELECT status, dispatch_state FROM platform_deploy_requests WHERE id='dispatching'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(dispatching.get::<String, _>("status"), "expired");
        assert_eq!(dispatching.get::<String, _>("dispatch_state"), "expired");
        assert!(
            !finish_dispatch_attempt(&pool, "dispatching", 1, "queued", "queued")
                .await
                .unwrap()
        );

        let pending = sqlx::query(
            "SELECT status, dispatch_state FROM platform_deploy_requests WHERE id='pending'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(pending.get::<String, _>("status"), "expired");
        assert_eq!(pending.get::<String, _>("dispatch_state"), "not_attempted");

        let future = sqlx::query(
            "SELECT status, dispatch_state FROM platform_deploy_requests WHERE id='future'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(future.get::<String, _>("status"), "approved");
        assert_eq!(future.get::<String, _>("dispatch_state"), "queued");

        let executing =
            sqlx::query("SELECT status FROM platform_deploy_requests WHERE id='executing'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(executing.get::<String, _>("status"), "executing");
    }

    #[tokio::test]
    async fn helper_failure_keeps_saved_approval_and_records_retryable_failure() {
        let pool = pool_with_approved_request().await;
        let attempt = begin_dispatch_attempt(&pool, "r1", 3, 100)
            .await
            .unwrap()
            .unwrap();
        finish_dispatch_attempt(&pool, "r1", attempt, "failed", "helper rejected")
            .await
            .unwrap();
        let row = sqlx::query("SELECT status, version, dispatch_state, dispatch_detail FROM platform_deploy_requests WHERE id='r1'")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(row.get::<String, _>("status"), "approved");
        assert_eq!(row.get::<i64, _>("version"), 3);
        assert_eq!(row.get::<String, _>("dispatch_state"), "failed");
        assert!(retry_eligible(
            row.get("status"),
            row.get("dispatch_state"),
            Some(101),
            102
        ));
    }

    #[tokio::test]
    async fn completion_does_not_overwrite_worker_lifecycle_and_fresh_attempt_is_exclusive() {
        let pool = pool_with_approved_request().await;
        let attempt = begin_dispatch_attempt(&pool, "r1", 3, 100)
            .await
            .unwrap()
            .unwrap();
        assert!(begin_dispatch_attempt(&pool, "r1", 3, 101)
            .await
            .unwrap()
            .is_none());
        sqlx::query(
            "UPDATE platform_deploy_requests SET status='executing', version=4 WHERE id='r1'",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            finish_dispatch_attempt(&pool, "r1", attempt, "queued", "queued")
                .await
                .unwrap()
        );
        let row = sqlx::query(
            "SELECT status, version, dispatch_state FROM platform_deploy_requests WHERE id='r1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.get::<String, _>("status"), "executing");
        assert_eq!(row.get::<i64, _>("version"), 4);
        assert_eq!(row.get::<String, _>("dispatch_state"), "queued");
    }

    #[tokio::test]
    async fn stale_dispatching_attempt_can_be_reclaimed_and_unknown_is_retryable() {
        let pool = pool_with_approved_request().await;
        let first = begin_dispatch_attempt(&pool, "r1", 3, 100)
            .await
            .unwrap()
            .unwrap();
        normalize_stale_dispatches(&pool, 130).await.unwrap();
        let row = sqlx::query(
            "SELECT dispatch_state, dispatch_detail FROM platform_deploy_requests WHERE id='r1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.get::<String, _>("dispatch_state"), "unknown");
        assert!(row
            .get::<String, _>("dispatch_detail")
            .contains("stopped responding"));
        assert!(begin_dispatch_attempt(&pool, "r1", 3, 130)
            .await
            .unwrap()
            .is_some());
        assert!(
            !finish_dispatch_attempt(&pool, "r1", first, "queued", "late")
                .await
                .unwrap()
        );
        assert!(retry_eligible(
            "approved".into(),
            "unknown".into(),
            None,
            140
        ));
        assert!(!retry_eligible(
            "executing".into(),
            "failed".into(),
            None,
            140
        ));
    }
}

async fn invoke_platform_ops(
    operation: &impl Serialize,
    expected_id: &str,
    expected_success_state: &str,
) -> Result<PlatformOperationResult, DispatchFailure> {
    let operation_path = std::env::var_os("VAPOR_PLATFORM_OPS_PATH")
        .unwrap_or_else(|| "/usr/local/libexec/vapor-platform-ops".into());
    let mut child = platform_ops_command(operation_path)
        .spawn()
        .map_err(|_| DispatchFailure {
            state: "failed",
            category: "helper_start_failed",
            technical: "failed to start fixed platform helper",
            public_message: "Approval is saved, but the deployment helper could not be started.",
        })?;
    let bytes = encode_platform_ops_request(operation)?;
    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        stdin.write_all(&bytes).await.map_err(|_| DispatchFailure { state: "unknown", category: "helper_request_write_failed", technical: "could not write helper request", public_message: "Approval is saved, but the dispatch result is uncertain. Reload the request before retrying." })?;
    } else {
        return Err(DispatchFailure {
            state: "failed",
            category: "helper_stdin_unavailable",
            technical: "helper stdin was unavailable",
            public_message: "Approval is saved, but the deployment helper could not be started.",
        });
    }
    let result = tokio::time::timeout(Duration::from_secs(10), child.wait_with_output())
        .await
        .map_err(|_| DispatchFailure { state: "unknown", category: "helper_timeout", technical: "helper wait exceeded ten-second timeout", public_message: "Approval is saved, but the dispatch result is uncertain. Reload the request before retrying." })?
        .map_err(|_| DispatchFailure { state: "unknown", category: "helper_wait_failed", technical: "could not collect helper result", public_message: "Approval is saved, but the dispatch result is uncertain. Reload the request before retrying." })?;
    if !result.status.success() || result.stdout.len() > 4096 {
        if result.stdout.len() <= 4096 {
            if let Ok(helper_result) =
                serde_json::from_slice::<PlatformOperationResult>(&result.stdout)
            {
                if helper_interactive_argv_mismatch(&helper_result) {
                    eprintln!(
                        "platform helper rejected request: category=helper_protocol_mismatch"
                    );
                    return Err(DispatchFailure {
                        state: "failed",
                        category: "helper_protocol_mismatch",
                        technical: "installed helper treated the stdin request as interactive argv",
                        public_message: "The installed Platform operation helper uses a different request protocol. Update Platform and its helper, then retry.",
                    });
                }
                if helper_result.operation_id == expected_id
                    && !helper_result.ok
                    && helper_result.state == "failed"
                    && !helper_result.detail.is_empty()
                    && helper_result.detail.len() <= 256
                    && !helper_result.detail.chars().any(char::is_control)
                {
                    eprintln!(
                        "platform helper rejected operation: {}",
                        helper_result.detail
                    );
                }
            }
        }
        return Err(DispatchFailure {
            state: "failed",
            category: "helper_rejected",
            technical: "fixed helper rejected or failed operation",
            public_message:
                "Approval is saved, but the deployment helper did not queue the deployment.",
        });
    }
    let value: PlatformOperationResult = serde_json::from_slice(&result.stdout).map_err(|_| DispatchFailure { state: "unknown", category: "helper_response_invalid", technical: "helper returned invalid JSON", public_message: "Approval is saved, but the dispatch result is uncertain. Reload the request before retrying." })?;
    if value.operation_id != expected_id
        || value.detail.is_empty()
        || value.detail.len() > 256
        || value.detail.chars().any(char::is_control)
        || (value.ok && value.state != expected_success_state)
        || (!value.ok && value.state != "failed")
    {
        return Err(DispatchFailure {
            state: "unknown",
            category: "helper_response_mismatch",
            technical: "fixed helper returned a mismatched operation result",
            public_message: "Approval is saved, but the dispatch result is uncertain. Reload the request before retrying.",
        });
    }
    if value.ok {
        Ok(value)
    } else {
        Err(DispatchFailure {
            state: "failed",
            category: "helper_rejected",
            technical: "fixed helper rejected operation",
            public_message:
                "Approval is saved, but the deployment helper did not queue the deployment.",
        })
    }
}

fn platform_ops_command(operation_path: std::ffi::OsString) -> tokio::process::Command {
    let mut command = tokio::process::Command::new("/usr/bin/sudo");
    command
        .arg("-n")
        .arg(operation_path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    command
}

fn helper_interactive_argv_mismatch(result: &PlatformOperationResult) -> bool {
    !result.ok
        && result.operation_id.is_empty()
        && result.state == "failed"
        && result.detail == "arguments are not accepted for interactive operations"
}

fn encode_platform_ops_request(operation: &impl Serialize) -> Result<Vec<u8>, DispatchFailure> {
    let bytes = serde_json::to_vec(operation).map_err(|_| DispatchFailure {
        state: "failed",
        category: "helper_request_encoding_failed",
        technical: "could not encode helper request",
        public_message: "Approval is saved, but the deployment request could not be prepared.",
    })?;
    if bytes.len() > 4096 {
        return Err(DispatchFailure {
            state: "failed",
            category: "helper_request_too_large",
            technical: "helper request exceeded size limit",
            public_message: "Approval is saved, but the deployment request could not be prepared.",
        });
    }
    Ok(bytes)
}

#[cfg(test)]
mod platform_ops_contract_tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn helper_is_called_without_operation_arguments() {
        let command = platform_ops_command(OsString::from("/test/vapor-platform-ops"));
        let args = command
            .as_std()
            .get_args()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(args, ["-n", "/test/vapor-platform-ops"]);
    }

    #[test]
    fn operation_payload_is_bounded_json_for_stdin() {
        #[derive(Serialize)]
        struct Request<'a> {
            operation: &'static str,
            request_id: &'a str,
        }
        let bytes = encode_platform_ops_request(&Request {
            operation: "dispatch_deploy",
            request_id: "request-123",
        })
        .unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
            serde_json::json!({"operation":"dispatch_deploy","request_id":"request-123"})
        );
        assert!(bytes.len() <= 4096);
        assert!(encode_platform_ops_request(&"x".repeat(5000)).is_err());
    }

    #[tokio::test]
    async fn failed_dispatch_response_exposes_only_the_static_safe_message() {
        let error = DispatchFailure {
            state: "failed",
            category: "helper_rejected",
            technical: "host-secret diagnostic must not reach the browser",
            public_message:
                "Approval is saved, but the deployment helper did not queue the deployment.",
        };
        let response = dispatch_failure_response(&error, true);
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains("dispatch_failed"));
        assert!(body.contains("Approval is saved"));
        assert!(!body.contains("host-secret"));
        assert!(body.contains("\"retry_eligible\":true"));
    }

    #[test]
    fn legacy_interactive_argv_rejection_is_reported_as_helper_mismatch() {
        let result = PlatformOperationResult {
            ok: false,
            operation_id: String::new(),
            state: "failed".to_owned(),
            detail: "arguments are not accepted for interactive operations".to_owned(),
        };
        assert!(helper_interactive_argv_mismatch(&result));
        let mut wrong_id = result;
        wrong_id.operation_id = "unexpected-request".to_owned();
        assert!(!helper_interactive_argv_mismatch(&wrong_id));
    }

    #[tokio::test]
    async fn helper_protocol_mismatch_has_a_specific_safe_response() {
        let error = DispatchFailure {
            state: "failed",
            category: "helper_protocol_mismatch",
            technical: "installed helper treated the stdin request as interactive argv",
            public_message: "The installed Platform operation helper uses a different request protocol. Update Platform and its helper, then retry.",
        };
        let response = dispatch_failure_response(&error, true);
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains("helper_protocol_mismatch"));
        assert!(body.contains("different request protocol"));
        assert!(!body.contains("installed helper treated"));
    }
}

fn valid_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok()
}

async fn verify_request_credential(token: &str) -> Result<IdentityCredentialDecision, Response> {
    verify_platform_request_credential(token, "platform:deploy-request:create")
        .await
        .map_err(|status| {
            if status == StatusCode::UNAUTHORIZED {
                (
                    status,
                    "valid Platform deploy-request credential required\n",
                )
                    .into_response()
            } else {
                status.into_response()
            }
        })
}

pub(crate) async fn verify_platform_request_credential(
    token: &str,
    scope: &str,
) -> Result<IdentityCredentialDecision, StatusCode> {
    if !matches!(
        scope,
        "platform:deploy-request:create" | "platform:ssh-access:request"
    ) {
        return Err(StatusCode::FORBIDDEN);
    }
    let service_token = std::env::var("VAPOR_PLATFORM_IDENTITY_REQUEST_AUTHZ_TOKEN")
        .ok()
        .filter(|value| valid_service_token(value))
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let endpoint =
        std::env::var("VAPOR_PLATFORM_IDENTITY_REQUEST_AUTHZ_URL").unwrap_or_else(|_| {
            "http://127.0.0.1:7110/api/identity/v1/internal/automation/verify".to_string()
        });
    if !crate::admin_auth::valid_loopback_endpoint(&endpoint) {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let response = client
        .post(&endpoint)
        .bearer_auth(service_token)
        .json(&serde_json::json!({
            "audience": "vapor-platform-server",
            "scope": scope,
            "credential_token": token,
        }))
        .send()
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    if response.status() != StatusCode::OK {
        return Err(if response.status() == StatusCode::UNAUTHORIZED {
            StatusCode::UNAUTHORIZED
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        });
    }
    let body = read_bounded_identity_response(response).await?;
    let verified: IdentityCredentialDecision =
        serde_json::from_slice(&body).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    if verified.scope != scope || verified.actor_profile_id.is_empty() {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(verified)
}

async fn read_bounded_identity_response(
    mut response: reqwest::Response,
) -> Result<Vec<u8>, StatusCode> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_IDENTITY_CREDENTIAL_RESPONSE_BYTES as u64)
    {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
    {
        if body.len().saturating_add(chunk.len()) > MAX_IDENTITY_CREDENTIAL_RESPONSE_BYTES {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

async fn require_root(state: &DeployRequestState, headers: &HeaderMap) -> Result<String, Response> {
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

fn same_origin(state: &DeployRequestState, headers: &HeaderMap) -> bool {
    headers.get("origin").and_then(|value| value.to_str().ok())
        == Some(state.public_origin.as_str())
}

fn validate_request(request: &CreateDeployRequest) -> Result<(), &'static str> {
    if !matches!(request.server_mode.as_str(), "monolith" | "services") {
        return Err("server_mode must be 'monolith' or 'services'\n");
    }
    match (request.deployment_scope.as_str(), request.service_target.as_deref()) {
        ("full", None) => {}
        ("service", Some(target)) if request.server_mode == "services" => {
            if !matches!(
                target,
                "atlas" | "homepage" | "docs" | "identity" | "diagnostics" | "registry" | "artifact" | "build-cache"
            ) {
                return Err("service_target is not an allowlisted deployment target\n");
            }
        }
        ("service", Some(_)) => {
            return Err("service scope requires server_mode='services'\n");
        }
        ("service", None) => return Err("service scope requires service_target\n"),
        ("full", Some(_)) => return Err("full scope must not have service_target\n"),
        _ => return Err("deployment_scope must be 'full' or 'service'\n"),
    }
    if request.repository != SOURCE_REPOSITORY || request.branch != DEPLOY_BRANCH {
        return Err("unsupported deployment source\n");
    }
    if !valid_hash(&request.commit) || !valid_hash(&request.tree) {
        return Err("commit and tree must be full 40-character hashes\n");
    }
    if request.submodules.len() != SUBMODULES.len() {
        return Err("request must include every pinned service submodule\n");
    }
    let mut actual = request
        .submodules
        .iter()
        .map(|item| item.path.as_str())
        .collect::<Vec<_>>();
    actual.sort_unstable();
    if actual != SUBMODULES {
        return Err("submodule paths do not match the Platform composition\n");
    }
    if request
        .submodules
        .iter()
        .any(|item| !valid_hash(&item.commit))
    {
        return Err("submodule pins must be full 40-character hashes\n");
    }
    if request.workflow_url.len() > 500
        || !request
            .workflow_url
            .starts_with("https://github.com/GHF-Studios/Vapor-Platform-Server/actions/")
    {
        return Err("workflow URL is not a Platform Actions URL\n");
    }
    if request.summary.len() > 500 || request.summary.chars().any(char::is_control) {
        return Err("invalid CI summary\n");
    }
    Ok(())
}

fn request_server_mode(request_json: &str) -> Option<String> {
    let request: CreateDeployRequest = serde_json::from_str(request_json).ok()?;
    validate_request(&request).ok()?;
    Some(request.server_mode)
}

fn request_scope(request_json: &str) -> Option<String> {
    let request: CreateDeployRequest = serde_json::from_str(request_json).ok()?;
    validate_request(&request).ok()?;
    Some(request.deployment_scope)
}

fn request_service_target(request_json: &str) -> Option<String> {
    let request: CreateDeployRequest = serde_json::from_str(request_json).ok()?;
    validate_request(&request).ok()?;
    request.service_target
}

fn valid_hash(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn valid_service_token(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
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
    use super::{
        read_bounded_identity_response, request_server_mode, request_scope,
        request_service_target, same_request_shape, validate_request, CreateDeployRequest,
        IdentityCredentialDecision, SubmodulePin, SUBMODULES,
    };
    use axum::routing::get;
    use axum::Router;
    use tokio::net::TcpListener;

    fn valid_request() -> CreateDeployRequest {
        CreateDeployRequest {
            server_mode: "services".to_string(),
            deployment_scope: "full".to_string(),
            service_target: None,
            repository: "GHF-Studios/Vapor-Platform-Server".to_string(),
            branch: "main".to_string(),
            commit: "0123456789abcdef0123456789abcdef01234567".to_string(),
            tree: "1123456789abcdef0123456789abcdef01234567".to_string(),
            submodules: SUBMODULES
                .iter()
                .map(|path| SubmodulePin {
                    path: (*path).to_string(),
                    commit: "2123456789abcdef0123456789abcdef01234567".to_string(),
                })
                .collect(),
            workflow_url: "https://github.com/GHF-Studios/Vapor-Platform-Server/actions/runs/1"
                .to_string(),
            summary: "Composed Platform CI passed.".to_string(),
        }
    }

    #[test]
    fn accepts_only_full_revision_with_every_service_pin() {
        assert!(validate_request(&valid_request()).is_ok());
    }

    #[test]
    fn rejects_missing_or_unknown_deployment_target() {
        let mut request = valid_request();
        request.server_mode.clear();
        assert!(validate_request(&request).is_err());

        let mut request = valid_request();
        request.server_mode = "automatic".to_string();
        assert!(validate_request(&request).is_err());
    }

    #[test]
    fn accepts_all_allowlisted_service_targets_and_rejects_bad_scope_shapes() {
        for target in [
            "atlas",
            "homepage",
            "docs",
            "identity",
            "diagnostics",
            "registry",
            "artifact",
            "build-cache",
        ] {
            let mut request = valid_request();
            request.deployment_scope = "service".to_string();
            request.service_target = Some(target.to_string());
            assert!(validate_request(&request).is_ok(), "target {target}");
        }

        let mut request = valid_request();
        request.deployment_scope = "service".to_string();
        assert!(validate_request(&request).is_err());

        let mut request = valid_request();
        request.service_target = Some("identity".to_string());
        assert!(validate_request(&request).is_err());

        let mut request = valid_request();
        request.deployment_scope = "service".to_string();
        request.service_target = Some("arbitrary-command".to_string());
        assert!(validate_request(&request).is_err());

        let mut request = valid_request();
        request.server_mode = "monolith".to_string();
        request.deployment_scope = "service".to_string();
        request.service_target = Some("identity".to_string());
        assert!(validate_request(&request).is_err());
    }

    #[test]
    fn request_summary_selection_fails_closed_for_old_or_invalid_bodies() {
        let full = serde_json::to_string(&valid_request()).unwrap();
        assert_eq!(request_scope(&full), Some("full".to_string()));
        assert_eq!(request_service_target(&full), None);

        let mut scoped = valid_request();
        scoped.deployment_scope = "service".to_string();
        scoped.service_target = Some("identity".to_string());
        let scoped = serde_json::to_string(&scoped).unwrap();
        assert_eq!(request_scope(&scoped), Some("service".to_string()));
        assert_eq!(request_service_target(&scoped), Some("identity".to_string()));

        assert_eq!(request_scope(r#"{"server_mode":"services"}"#), None);
        assert_eq!(request_service_target(r#"{"server_mode":"services"}"#), None);
    }

    #[test]
    fn active_request_is_idempotent_only_for_the_same_deployment_shape() {
        let existing = serde_json::to_string(&valid_request()).unwrap();
        let same_commit_different_scope = {
            let mut incoming = valid_request();
            incoming.deployment_scope = "service".to_string();
            incoming.service_target = Some("identity".to_string());
            incoming
        };
        assert!(!same_request_shape(&existing, &same_commit_different_scope));

        let same_shape_different_ci_run = {
            let mut incoming = valid_request();
            incoming.workflow_url = "https://github.com/GHF-Studios/Vapor-Platform-Server/actions/runs/2".to_string();
            incoming.summary = "Another CI run for the same deployment shape.".to_string();
            incoming
        };
        assert!(same_request_shape(&existing, &same_shape_different_ci_run));
    }

    #[test]
    fn parses_legacy_request_without_a_deployment_target_as_unspecified() {
        let legacy = r#"{"repository":"GHF-Studios/Vapor-Platform-Server"}"#;
        assert_eq!(request_server_mode(legacy), None);
        assert_eq!(
            request_server_mode(r#"{"server_mode":"services"}"#),
            None
        );
    }

    #[test]
    fn rejects_branch_substitution_and_incomplete_or_malformed_pins() {
        let mut request = valid_request();
        request.branch = "attacker-branch".to_string();
        assert!(validate_request(&request).is_err());

        let mut request = valid_request();
        request.submodules.pop();
        assert!(validate_request(&request).is_err());

        let mut request = valid_request();
        request.submodules[0].commit = "not-a-full-commit".to_string();
        assert!(validate_request(&request).is_err());
    }

    #[test]
    fn rejects_untrusted_repository_and_workflow_links() {
        let mut request = valid_request();
        request.repository = "attacker/other-repo".to_string();
        assert!(validate_request(&request).is_err());

        let mut request = valid_request();
        request.workflow_url = "https://attacker.example/actions".to_string();
        assert!(validate_request(&request).is_err());
    }

    #[tokio::test]
    async fn identity_credential_response_is_bounded() {
        let app = Router::new().route("/credential", get(|| async { "x".repeat(4097) }));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let response = reqwest::Client::new()
            .get(format!("http://{address}/credential"))
            .send()
            .await
            .unwrap();

        assert!(read_bounded_identity_response(response).await.is_err());
        server.abort();
    }

    #[test]
    fn identity_credential_response_rejects_unknown_fields() {
        let valid = br#"{"credential_id":"credential-1","actor_profile_id":"profile-1","scope":"platform:deploy-request:create","bound_ssh_fingerprint":null,"bound_ssh_request_hash":null}"#;
        assert!(serde_json::from_slice::<IdentityCredentialDecision>(valid).is_ok());

        let unexpected = br#"{"credential_id":"credential-1","actor_profile_id":"profile-1","scope":"platform:deploy-request:create","bound_ssh_fingerprint":null,"bound_ssh_request_hash":null,"is_root":true}"#;
        assert!(serde_json::from_slice::<IdentityCredentialDecision>(unexpected).is_err());
    }
}
