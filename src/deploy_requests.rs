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
    identity_request_authz_url: String,
    identity_request_authz_token: Option<String>,
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
    repository: String,
    branch: String,
    commit: String,
    tree: String,
    submodules: Vec<SubmodulePin>,
    workflow_url: String,
    summary: String,
    status: String,
    version: i64,
    created_at_unix: i64,
    expires_at_unix: i64,
    decided_at_unix: Option<i64>,
    decision_reason: Option<String>,
    result_detail: Option<String>,
}

#[derive(Serialize)]
struct CreatedRequest {
    id: String,
    status: String,
    version: i64,
    expires_at_unix: i64,
}

#[derive(Deserialize)]
struct IdentityCredentialDecision {
    credential_id: String,
    actor_profile_id: String,
    scope: String,
}

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
        identity_request_authz_url: std::env::var("VAPOR_PLATFORM_IDENTITY_REQUEST_AUTHZ_URL")
            .unwrap_or_else(|_| {
                "http://127.0.0.1:7110/api/identity/v1/internal/automation/verify".to_string()
            }),
        identity_request_authz_token: std::env::var("VAPOR_PLATFORM_IDENTITY_REQUEST_AUTHZ_TOKEN")
            .ok(),
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
    let credential = match verify_request_credential(&state, token).await {
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
    if let Some(existing) = active_request_for_commit(&state.pool, &request.commit).await {
        return (StatusCode::ACCEPTED, Json(existing)).into_response();
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
        if let Some(existing) = active_request_for_commit(&state.pool, &request.commit).await {
            return (StatusCode::ACCEPTED, Json(existing)).into_response();
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

async fn active_request_for_commit(pool: &SqlitePool, commit: &str) -> Option<CreatedRequest> {
    let row = sqlx::query(
        "SELECT id, status, version, expires_at_unix FROM platform_deploy_requests
         WHERE commit_hash = ? AND status IN ('pending','approved','executing') LIMIT 1",
    )
    .bind(commit)
    .fetch_optional(pool)
    .await
    .ok()??;
    Some(CreatedRequest {
        id: row.get("id"),
        status: row.get("status"),
        version: row.get("version"),
        expires_at_unix: row.get("expires_at_unix"),
    })
}

async fn list_requests(State(state): State<DeployRequestState>, headers: HeaderMap) -> Response {
    let owner = match require_root(&state, &headers).await {
        Ok(_) => None,
        Err(root_failure) => {
            let Some(token) = bearer_token(&headers) else {
                return root_failure;
            };
            match verify_request_credential(&state, token).await {
                Ok(credential) => Some(credential.credential_id),
                Err(_) => return root_failure,
            }
        }
    };
    if expire_requests(&state.pool, unix_now()).await.is_err() {
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
                repository: row.get("repository"),
                branch: row.get("branch"),
                commit: row.get("commit_hash"),
                tree: row.get("tree_hash"),
                submodules,
                workflow_url: row.get("workflow_url"),
                summary: row.get("summary"),
                status: row.get("status"),
                version: row.get("version"),
                created_at_unix: row.get("created_at_unix"),
                expires_at_unix: row.get("expires_at_unix"),
                decided_at_unix: row.get("decided_at_unix"),
                decision_reason: row.get("decision_reason"),
                result_detail: row.get("result_detail"),
            })
        })
        .collect::<Vec<_>>();
    Json(requests).into_response()
}

async fn fetch_request_rows(
    pool: &SqlitePool,
    owner_credential_id: Option<&str>,
) -> Result<Vec<sqlx::sqlite::SqliteRow>, sqlx::Error> {
    let query = "SELECT id, repository, branch, commit_hash, tree_hash, submodules_json, workflow_url,
                        summary, status, version, created_at_unix, expires_at_unix, decided_at_unix, decision_reason, result_detail
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
         decided_at_unix = ? WHERE status IN ('pending','approved') AND expires_at_unix <= ?",
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
        Err(response) => return response,
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
        Ok(result) if result.rows_affected() == 1 => (StatusCode::OK, Json(serde_json::json!({"id": id, "status": next, "version": request.expected_version + 1}))).into_response(),
        Ok(_) => (StatusCode::CONFLICT, "request is expired or no longer pending at that version\n").into_response(),
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

async fn verify_request_credential(
    state: &DeployRequestState,
    token: &str,
) -> Result<IdentityCredentialDecision, Response> {
    let service_token = state
        .identity_request_authz_token
        .as_deref()
        .filter(|value| valid_service_token(value))
        .ok_or_else(|| StatusCode::SERVICE_UNAVAILABLE.into_response())?;
    let response = state
        .client
        .post(&state.identity_request_authz_url)
        .bearer_auth(service_token)
        .json(&serde_json::json!({
            "audience": "vapor-platform-server",
            "scope": "platform:deploy-request:create",
            "credential_token": token,
        }))
        .send()
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE.into_response())?;
    if response.status() != StatusCode::OK {
        return Err((
            StatusCode::UNAUTHORIZED,
            "valid Platform deploy-request credential required\n",
        )
            .into_response());
    }
    let verified: IdentityCredentialDecision = response
        .json()
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE.into_response())?;
    if verified.scope != "platform:deploy-request:create" || verified.actor_profile_id.is_empty() {
        return Err(StatusCode::FORBIDDEN.into_response());
    }
    Ok(verified)
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
fn sha256_hex(bytes: &[u8]) -> String {
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
    use super::{validate_request, CreateDeployRequest, SubmodulePin, SUBMODULES};

    fn valid_request() -> CreateDeployRequest {
        CreateDeployRequest {
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
}
