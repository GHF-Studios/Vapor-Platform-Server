use crate::server_fleet::ServerNode;
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::header::CACHE_CONTROL;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

const DEFAULT_IDENTITY_AUTHZ_URL: &str =
    "http://127.0.0.1:7110/api/identity/v1/internal/authorization/check";
const REPORT_MIN_INTERVAL_SECONDS: i64 = 10;
const REPORT_FRESHNESS_SECONDS: i64 = 90;
const MAX_CLOCK_SKEW_SECONDS: i64 = 60;
const MAX_AGENT_VERSION_BYTES: usize = 64;
const ROLE_ALLOWLIST: &[&str] = &["control-plane", "builder", "backup", "general"];
static NODE_STATE: OnceLock<NodeState> = OnceLock::new();

#[derive(Clone)]
struct NodeState {
    pool: SqlitePool,
    authz_endpoint: String,
    service_token: Option<String>,
    client: reqwest::Client,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NetworkQuery {
    window: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateNodeRequest {
    label: String,
    role: String,
    #[serde(default)]
    slot_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RevokeNodeRequest {
    expected_version: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TelemetryReport {
    schema_version: u32,
    sampled_at_unix: i64,
    cpu_percent: f64,
    logical_cpu_count: i64,
    memory_total_bytes: i64,
    memory_available_bytes: i64,
    uptime_seconds: i64,
    load_average_1m: f64,
    agent_version: String,
    network: Option<crate::network_telemetry::NetworkSnapshot>,
    services: Vec<crate::service_telemetry::ServiceMetrics>,
}

#[derive(Serialize)]
struct NetworkHistorySample {
    sampled_at_unix: i64,
    rx_bytes: i64,
    tx_bytes: i64,
    rx_errors: i64,
    tx_errors: i64,
    rx_dropped: i64,
    tx_dropped: i64,
    tcp_states: serde_json::Value,
}

#[derive(Serialize)]
struct NetworkResponse {
    schema_version: u32,
    node_id: String,
    observed_at_unix: Option<i64>,
    network: Option<crate::network_telemetry::NetworkSnapshot>,
    samples: Vec<NetworkHistorySample>,
}

#[derive(Serialize)]
struct CreatedNode {
    id: String,
    slot_id: String,
    label: String,
    role: String,
    status: String,
    version: i64,
    reporter_credential: String,
    executor_credential: String,
    warning: &'static str,
}

#[derive(Serialize)]
struct NodeRecord {
    id: String,
    slot_id: String,
    label: String,
    role: String,
    status: String,
    version: i64,
    created_at_unix: i64,
    created_by_profile_id: String,
    revoked_at_unix: Option<i64>,
    revoked_by_profile_id: Option<String>,
    last_report_received_at_unix: Option<i64>,
    sampled_at_unix: Option<i64>,
    agent_version: Option<String>,
    executor_enabled: bool,
    executor_online: bool,
}

#[derive(Serialize)]
struct NodeEvent {
    id: i64,
    node_id: String,
    event: String,
    label: String,
    role: String,
    actor_profile_id: String,
    at_unix: i64,
}

#[derive(Serialize)]
struct NodeInventory {
    schema_version: u32,
    slots: Vec<ServerSlot>,
    nodes: Vec<NodeRecord>,
    events: Vec<NodeEvent>,
}

#[derive(Serialize)]
struct ServerSlot {
    id: String,
    label: String,
    role: String,
    status: String,
    version: i64,
    current_node_id: Option<String>,
    changed_at_unix: i64,
}

pub(crate) fn router() -> Router {
    router_with_state(state())
}

fn router_with_state(state: NodeState) -> Router {
    Router::new()
        .route("/api/admin/server-nodes", get(list_nodes).post(create_node))
        .route("/api/admin/server-nodes/{id}/network", get(node_network))
        .route("/api/admin/server-nodes/{id}/revoke", post(revoke_node))
        .route("/api/fleet/v1/nodes/{id}/telemetry", post(report_telemetry))
        .with_state(state)
        .layer(DefaultBodyLimit::max(16 * 1024))
        .route_layer(axum::middleware::from_fn(no_store))
}

pub(crate) async fn initialize() -> Result<(), Box<dyn std::error::Error>> {
    let directory = state_directory();
    tokio::fs::create_dir_all(directory).await?;
    let pool = create_pool();
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS platform_server_nodes (
            id TEXT PRIMARY KEY,
            slot_id TEXT,
            label TEXT NOT NULL,
            role TEXT NOT NULL CHECK(role IN ('control-plane','builder','backup','general')),
            status TEXT NOT NULL CHECK(status IN ('active','revoked')),
            credential_sha256 TEXT NOT NULL,
            executor_credential_sha256 TEXT,
            executor_last_seen_at_unix INTEGER,
            version INTEGER NOT NULL,
            created_at_unix INTEGER NOT NULL,
            created_by_profile_id TEXT NOT NULL,
            revoked_at_unix INTEGER,
            revoked_by_profile_id TEXT,
            last_report_received_at_unix INTEGER,
            sampled_at_unix INTEGER,
            cpu_percent REAL,
            logical_cpu_count INTEGER,
            memory_total_bytes INTEGER,
            memory_available_bytes INTEGER,
            uptime_seconds INTEGER,
            load_average_1m REAL,
            agent_version TEXT,
            network_json TEXT,
            network_sampled_at_unix INTEGER,
            services_json TEXT,
            services_sampled_at_unix INTEGER
        )",
    )
    .execute(&pool)
    .await?;
    let columns = sqlx::query("PRAGMA table_info(platform_server_nodes)")
        .fetch_all(&pool)
        .await?;
    if !columns
        .iter()
        .any(|row| row.get::<String, _>("name") == "network_json")
    {
        sqlx::query("ALTER TABLE platform_server_nodes ADD COLUMN network_json TEXT")
            .execute(&pool)
            .await?;
    }
    if !columns
        .iter()
        .any(|row| row.get::<String, _>("name") == "network_sampled_at_unix")
    {
        sqlx::query("ALTER TABLE platform_server_nodes ADD COLUMN network_sampled_at_unix INTEGER")
            .execute(&pool)
            .await?;
    }
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS platform_server_network_samples (
            node_id TEXT NOT NULL,
            sampled_at_unix INTEGER NOT NULL,
            rx_bytes INTEGER NOT NULL,
            tx_bytes INTEGER NOT NULL,
            rx_errors INTEGER NOT NULL,
            tx_errors INTEGER NOT NULL,
            rx_dropped INTEGER NOT NULL,
            tx_dropped INTEGER NOT NULL,
            tcp_states_json TEXT NOT NULL,
            PRIMARY KEY(node_id,sampled_at_unix)
        )",
    )
    .execute(&pool)
    .await?;
    sqlx::query("DELETE FROM platform_server_network_samples WHERE sampled_at_unix<?")
        .bind(unix_now().saturating_sub(7 * 24 * 60 * 60))
        .execute(&pool)
        .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_platform_server_network_samples_time
         ON platform_server_network_samples(node_id,sampled_at_unix DESC)",
    )
    .execute(&pool)
    .await?;
    initialize_slots(&pool).await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_platform_server_nodes_status_label
         ON platform_server_nodes(status,label,id)",
    )
    .execute(&pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS platform_server_node_events (
            event_id INTEGER PRIMARY KEY AUTOINCREMENT,
            node_id TEXT NOT NULL,
            event TEXT NOT NULL CHECK(event IN ('created','revoked')),
            label TEXT NOT NULL,
            role TEXT NOT NULL CHECK(role IN ('control-plane','builder','backup','general')),
            actor_profile_id TEXT NOT NULL,
            at_unix INTEGER NOT NULL
        )",
    )
    .execute(&pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_platform_server_node_events_time
         ON platform_server_node_events(at_unix DESC,event_id DESC)",
    )
    .execute(&pool)
    .await?;
    let _ = NODE_STATE.set(NodeState {
        pool,
        authz_endpoint: std::env::var("VAPOR_PLATFORM_IDENTITY_AUTHZ_URL")
            .unwrap_or_else(|_| DEFAULT_IDENTITY_AUTHZ_URL.to_owned()),
        service_token: std::env::var("VAPOR_PLATFORM_IDENTITY_AUTHZ_TOKEN").ok(),
        client: reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(1))
            .timeout(Duration::from_secs(3))
            .redirect(reqwest::redirect::Policy::none())
            .build()?,
    });
    Ok(())
}

async fn initialize_slots(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let columns = sqlx::query("PRAGMA table_info(platform_server_nodes)")
        .fetch_all(pool)
        .await?;
    let has_slot_id = columns
        .iter()
        .any(|row| row.get::<String, _>("name") == "slot_id");
    if !has_slot_id {
        sqlx::query("ALTER TABLE platform_server_nodes ADD COLUMN slot_id TEXT")
            .execute(pool)
            .await?;
    }
    if !columns
        .iter()
        .any(|row| row.get::<String, _>("name") == "executor_credential_sha256")
    {
        sqlx::query("ALTER TABLE platform_server_nodes ADD COLUMN executor_credential_sha256 TEXT")
            .execute(pool)
            .await?;
    }
    let columns = sqlx::query("PRAGMA table_info(platform_server_nodes)")
        .fetch_all(pool)
        .await?;
    if !columns
        .iter()
        .any(|row| row.get::<String, _>("name") == "executor_last_seen_at_unix")
    {
        sqlx::query(
            "ALTER TABLE platform_server_nodes ADD COLUMN executor_last_seen_at_unix INTEGER",
        )
        .execute(pool)
        .await?;
    }
    if !columns
        .iter()
        .any(|row| row.get::<String, _>("name") == "services_json")
    {
        sqlx::query("ALTER TABLE platform_server_nodes ADD COLUMN services_json TEXT")
            .execute(pool)
            .await?;
    }
    if !columns
        .iter()
        .any(|row| row.get::<String, _>("name") == "services_sampled_at_unix")
    {
        sqlx::query(
            "ALTER TABLE platform_server_nodes ADD COLUMN services_sampled_at_unix INTEGER",
        )
        .execute(pool)
        .await?;
    }
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS platform_server_slots (
            id TEXT PRIMARY KEY,
            label TEXT NOT NULL,
            role TEXT NOT NULL CHECK(role IN ('control-plane','builder','backup','general')),
            status TEXT NOT NULL CHECK(status IN ('active','vacant','retired')),
            version INTEGER NOT NULL,
            created_at_unix INTEGER NOT NULL,
            created_by_profile_id TEXT NOT NULL,
            current_node_id TEXT,
            changed_at_unix INTEGER NOT NULL,
            changed_by_profile_id TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT OR IGNORE INTO platform_server_slots
         (id,label,role,status,version,created_at_unix,created_by_profile_id,current_node_id,changed_at_unix,changed_by_profile_id)
         SELECT id,label,role,
                CASE WHEN status='active' THEN 'active' ELSE 'vacant' END,
                1,created_at_unix,created_by_profile_id,
                CASE WHEN status='active' THEN id ELSE NULL END,
                COALESCE(revoked_at_unix,created_at_unix),
                COALESCE(revoked_by_profile_id,created_by_profile_id)
         FROM platform_server_nodes",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "UPDATE platform_server_nodes SET slot_id=id
         WHERE slot_id IS NULL OR slot_id=''",
    )
    .execute(pool)
    .await?;
    let local_slot_id = crate::server_operation_schema::local_slot_id();
    let local_node_id = crate::server_operation_schema::local_node_id();
    let now = unix_now();
    let local_label = std::env::var("VAPOR_PLATFORM_SERVER_SLOT_LABEL")
        .ok()
        .filter(|value| !value.trim().is_empty() && value.len() <= 80)
        .unwrap_or_else(|| "Core server".to_owned());
    sqlx::query(
        "INSERT OR IGNORE INTO platform_server_slots
         (id,label,role,status,version,created_at_unix,created_by_profile_id,current_node_id,changed_at_unix,changed_by_profile_id)
         VALUES (?,?,'control-plane','active',1,?,'platform-bootstrap',?,?,'platform-bootstrap')",
    )
    .bind(local_slot_id)
    .bind(local_label)
    .bind(now)
    .bind(local_node_id)
    .bind(now)
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_platform_server_slots_current_node
         ON platform_server_slots(current_node_id) WHERE current_node_id IS NOT NULL",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_platform_server_nodes_slot_history
         ON platform_server_nodes(slot_id,created_at_unix,id)",
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn fleet_nodes(now: i64) -> Result<Vec<ServerNode>, sqlx::Error> {
    let state = NODE_STATE.get().ok_or(sqlx::Error::PoolTimedOut)?;
    let rows = sqlx::query(
        "SELECT id,slot_id,label,role,status,created_at_unix,last_report_received_at_unix,
                sampled_at_unix,cpu_percent,logical_cpu_count,memory_total_bytes,
                memory_available_bytes,uptime_seconds,load_average_1m,network_json,
                services_json,services_sampled_at_unix
         FROM platform_server_nodes ORDER BY status,label,id",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let status: String = row.get("status");
            let last_received: Option<i64> = row.get("last_report_received_at_unix");
            let sampled_at: Option<i64> = row.get("sampled_at_unix");
            let online = status == "active"
                && last_received.is_some_and(|received| {
                    now.saturating_sub(received) <= REPORT_FRESHNESS_SECONDS
                })
                && sampled_at
                    .is_some_and(|sampled| now.saturating_sub(sampled) <= REPORT_FRESHNESS_SECONDS);
            let state = match status.as_str() {
                "revoked" => "revoked",
                _ if online => "online",
                _ if last_received.is_none() => "never-reported",
                _ => "stale",
            };
            let has_metrics: bool = row.get::<Option<f64>, _>("cpu_percent").is_some()
                && row.get::<Option<i64>, _>("memory_total_bytes").is_some()
                && row.get::<Option<i64>, _>("uptime_seconds").is_some();
            ServerNode {
                id: row.get("id"),
                slot_id: row
                    .get::<Option<String>, _>("slot_id")
                    .unwrap_or_else(|| row.get("id")),
                label: row.get("label"),
                role: row.get("role"),
                state,
                metrics_state: if has_metrics {
                    "complete"
                } else {
                    "unavailable"
                },
                observed_at_unix: last_received.unwrap_or_else(|| row.get("created_at_unix"))
                    as u64,
                cpu_percent: row.get("cpu_percent"),
                logical_cpu_count: row
                    .get::<Option<i64>, _>("logical_cpu_count")
                    .and_then(|value| usize::try_from(value).ok()),
                memory_total_bytes: row
                    .get::<Option<i64>, _>("memory_total_bytes")
                    .and_then(|value| u64::try_from(value).ok()),
                memory_available_bytes: row
                    .get::<Option<i64>, _>("memory_available_bytes")
                    .and_then(|value| u64::try_from(value).ok()),
                uptime_seconds: row
                    .get::<Option<i64>, _>("uptime_seconds")
                    .and_then(|value| u64::try_from(value).ok()),
                load_average_1m: row.get("load_average_1m"),
                network: row
                    .get::<Option<String>, _>("network_json")
                    .and_then(|value| serde_json::from_str(&value).ok())
                    .filter(crate::network_telemetry::valid_network_snapshot),
                service_metrics: row
                    .get::<Option<String>, _>("services_json")
                    .and_then(|value| {
                        serde_json::from_str::<Vec<crate::service_telemetry::ServiceMetrics>>(
                            &value,
                        )
                        .ok()
                    })
                    .filter(|metrics| crate::service_telemetry::valid_service_snapshot(metrics)),
                service_metrics_sampled_at_unix: row
                    .get::<Option<i64>, _>("services_sampled_at_unix")
                    .and_then(|value| u64::try_from(value).ok()),
            }
        })
        .collect())
}

async fn list_nodes(State(state): State<NodeState>, headers: HeaderMap) -> Response {
    let actor = match authorize_root(&state, &headers).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let nodes = sqlx::query(
        "SELECT id,slot_id,label,role,status,version,created_at_unix,created_by_profile_id,
                revoked_at_unix,revoked_by_profile_id,last_report_received_at_unix,
                sampled_at_unix,agent_version,
                (executor_credential_sha256 IS NOT NULL) AS executor_enabled,
                (executor_last_seen_at_unix IS NOT NULL AND executor_last_seen_at_unix>=?) AS executor_online
         FROM platform_server_nodes ORDER BY created_at_unix DESC,id DESC LIMIT 200",
    )
    .bind(unix_now().saturating_sub(REPORT_FRESHNESS_SECONDS))
    .fetch_all(&state.pool)
    .await;
    let events = sqlx::query(
        "SELECT event_id,node_id,event,label,role,actor_profile_id,at_unix
         FROM platform_server_node_events ORDER BY event_id DESC LIMIT 50",
    )
    .fetch_all(&state.pool)
    .await;
    let slots = sqlx::query(
        "SELECT id,label,role,status,version,current_node_id,changed_at_unix
         FROM platform_server_slots ORDER BY status,label,id",
    )
    .fetch_all(&state.pool)
    .await;
    match (nodes, events, slots) {
        (Ok(nodes), Ok(events), Ok(slots)) => private_json(
            StatusCode::OK,
            NodeInventory {
                schema_version: 1,
                slots: slots.into_iter().map(server_slot).collect(),
                nodes: nodes.into_iter().map(node_record).collect(),
                events: events.into_iter().map(node_event).collect(),
            },
        ),
        _ => {
            let _ = actor;
            error(
                StatusCode::SERVICE_UNAVAILABLE,
                "server_node_inventory_unavailable",
            )
        }
    }
}

async fn create_node(
    State(state): State<NodeState>,
    headers: HeaderMap,
    Json(request): Json<CreateNodeRequest>,
) -> Response {
    let actor = match authorize_root(&state, &headers).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    if !same_origin(&headers) {
        return error(StatusCode::FORBIDDEN, "same_origin_confirmation_required");
    }
    let label = request.label.trim();
    if label.is_empty() || label.chars().count() > 80 || label.chars().any(char::is_control) {
        return error(StatusCode::BAD_REQUEST, "invalid_server_label");
    }
    if !ROLE_ALLOWLIST.contains(&request.role.as_str()) {
        return error(StatusCode::BAD_REQUEST, "invalid_server_role");
    }
    let id = uuid::Uuid::new_v4().to_string();
    let new_slot = request.slot_id.is_none();
    let slot_id = request
        .slot_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    if uuid::Uuid::parse_str(&slot_id).is_err() {
        return error(StatusCode::BAD_REQUEST, "invalid_server_slot_id");
    }
    let credential = format!(
        "v1.{id}.{}.{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let credential_sha256 = sha256_hex(credential.as_bytes());
    let executor_credential = format!(
        "v1.{id}.{}.{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let executor_credential_sha256 = sha256_hex(executor_credential.as_bytes());
    let now = unix_now();
    let mut transaction = match state.pool.begin().await {
        Ok(transaction) => transaction,
        Err(_) => {
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "server_node_state_unavailable",
            );
        }
    };
    let slot_version = if new_slot {
        None
    } else {
        let slot =
            sqlx::query("SELECT status,version,label,role FROM platform_server_slots WHERE id=?")
                .bind(&slot_id)
                .fetch_optional(&mut *transaction)
                .await;
        match slot {
            Ok(Some(slot))
                if slot.get::<String, _>("status") == "vacant"
                    && slot.get::<String, _>("label") == label
                    && slot.get::<String, _>("role") == request.role =>
            {
                Some(slot.get::<i64, _>("version"))
            }
            Ok(Some(_)) => {
                return error(
                    StatusCode::CONFLICT,
                    "server_slot_not_vacant_or_details_changed",
                );
            }
            Ok(None) => return error(StatusCode::NOT_FOUND, "server_slot_not_found"),
            Err(_) => {
                return error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "server_node_state_unavailable",
                );
            }
        }
    };
    if let Some(version) = slot_version {
        let changed = sqlx::query(
            "UPDATE platform_server_slots SET status='active',current_node_id=?,version=version+1,
                    changed_at_unix=?,changed_by_profile_id=?
             WHERE id=? AND status='vacant' AND version=? AND current_node_id IS NULL",
        )
        .bind(&id)
        .bind(now)
        .bind(&actor)
        .bind(&slot_id)
        .bind(version)
        .execute(&mut *transaction)
        .await;
        if !changed.is_ok_and(|result| result.rows_affected() == 1) {
            return error(StatusCode::CONFLICT, "server_slot_changed_during_enrollment");
        }
    } else if sqlx::query(
        "INSERT INTO platform_server_slots
         (id,label,role,status,version,created_at_unix,created_by_profile_id,current_node_id,changed_at_unix,changed_by_profile_id)
         VALUES (?,?,?,'active',1,?,?,?,?,?)",
    )
    .bind(&slot_id)
    .bind(label)
    .bind(&request.role)
    .bind(now)
    .bind(&actor)
    .bind(&id)
    .bind(now)
    .bind(&actor)
    .execute(&mut *transaction)
    .await
    .is_err()
    {
        return error(StatusCode::CONFLICT, "server_slot_creation_conflict");
    }
    let inserted = sqlx::query(
        "INSERT INTO platform_server_nodes
         (id,slot_id,label,role,status,credential_sha256,executor_credential_sha256,version,created_at_unix,created_by_profile_id)
         VALUES (?,?,?,?,'active',?,?,1,?,?)",
    )
    .bind(&id)
    .bind(&slot_id)
    .bind(label)
    .bind(&request.role)
    .bind(&credential_sha256)
    .bind(&executor_credential_sha256)
    .bind(now)
    .bind(&actor)
    .execute(&mut *transaction)
    .await;
    if inserted.is_err()
        || insert_node_event(
            &mut transaction,
            &id,
            "created",
            label,
            &request.role,
            &actor,
            now,
        )
        .await
        .is_err()
        || transaction.commit().await.is_err()
    {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "server_node_state_unavailable",
        );
    }
    private_json(
        StatusCode::CREATED,
        CreatedNode {
            id,
            slot_id,
            label: label.to_owned(),
            role: request.role,
            status: "active".to_owned(),
            version: 1,
            reporter_credential: credential,
            executor_credential,
            warning: "Each credential is shown once. Telemetry cannot authorize actions; the separate executor credential only claims and reports root-operator-authored typed actions for this node.",
        },
    )
}

async fn revoke_node(
    State(state): State<NodeState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<RevokeNodeRequest>,
) -> Response {
    let actor = match authorize_root(&state, &headers).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    if !same_origin(&headers) || request.expected_version < 1 {
        return error(StatusCode::FORBIDDEN, "same_origin_confirmation_required");
    }
    let now = unix_now();
    let mut transaction = match state.pool.begin().await {
        Ok(transaction) => transaction,
        Err(_) => {
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "server_node_state_unavailable",
            );
        }
    };
    let row = sqlx::query(
        "SELECT slot_id,label,role FROM platform_server_nodes
         WHERE id=? AND status='active' AND version=?",
    )
    .bind(&id)
    .bind(request.expected_version)
    .fetch_optional(&mut *transaction)
    .await;
    let row = match row {
        Ok(Some(row)) => row,
        Ok(None) => {
            return error(
                StatusCode::CONFLICT,
                "server_node_changed_or_already_revoked",
            );
        }
        Err(_) => {
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "server_node_state_unavailable",
            );
        }
    };
    let label: String = row.get("label");
    let role: String = row.get("role");
    let slot_id: Option<String> = row.get("slot_id");
    let updated = sqlx::query(
        "UPDATE platform_server_nodes SET status='revoked',version=version+1,
                revoked_at_unix=?,revoked_by_profile_id=?
         WHERE id=? AND status='active' AND version=?",
    )
    .bind(now)
    .bind(&actor)
    .bind(&id)
    .bind(request.expected_version)
    .execute(&mut *transaction)
    .await;
    if !updated.is_ok_and(|result| result.rows_affected() == 1) {
        return error(
            StatusCode::CONFLICT,
            "server_node_changed_or_already_revoked",
        );
    }
    let slot_updated = sqlx::query(
        "UPDATE platform_server_slots SET status='vacant',current_node_id=NULL,
                version=version+1,changed_at_unix=?,changed_by_profile_id=?
         WHERE id=? AND current_node_id=? AND status='active'",
    )
    .bind(now)
    .bind(&actor)
    .bind(slot_id.as_deref().unwrap_or_default())
    .bind(&id)
    .execute(&mut *transaction)
    .await;
    if !slot_updated.is_ok_and(|result| result.rows_affected() == 1) {
        return error(StatusCode::CONFLICT, "server_slot_changed_or_missing");
    }
    if insert_node_event(&mut transaction, &id, "revoked", &label, &role, &actor, now)
        .await
        .is_err()
        || transaction.commit().await.is_err()
    {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "server_node_state_unavailable",
        );
    }
    private_json(
        StatusCode::OK,
        serde_json::json!({"id":id,"status":"revoked"}),
    )
}

async fn report_telemetry(
    State(state): State<NodeState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(report): Json<TelemetryReport>,
) -> Response {
    let credential = match bearer_credential(&headers, &id) {
        Some(value) => value,
        None => return error(StatusCode::UNAUTHORIZED, "reporter_credential_invalid"),
    };
    let now = unix_now();
    if !valid_report(&report, now) {
        return error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "telemetry_report_invalid_or_stale",
        );
    }
    let credential_sha256 = sha256_hex(credential.as_bytes());
    let network_json = report
        .network
        .as_ref()
        .and_then(|snapshot| serde_json::to_string(snapshot).ok());
    let services_json = serde_json::to_string(&report.services).ok();
    let mut transaction = match state.pool.begin().await {
        Ok(transaction) => transaction,
        Err(_) => {
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "server_node_state_unavailable",
            );
        }
    };
    let updated = sqlx::query(
        "UPDATE platform_server_nodes
         SET last_report_received_at_unix=?,sampled_at_unix=?,cpu_percent=?,
             logical_cpu_count=?,memory_total_bytes=?,memory_available_bytes=?,
             uptime_seconds=?,load_average_1m=?,agent_version=?,
             network_json=COALESCE(?,network_json),
             network_sampled_at_unix=CASE WHEN ? IS NULL THEN network_sampled_at_unix ELSE ? END,
             services_json=COALESCE(?,services_json),
             services_sampled_at_unix=CASE WHEN ? IS NULL THEN services_sampled_at_unix ELSE ? END
         WHERE id=? AND status='active' AND credential_sha256=?
           AND (last_report_received_at_unix IS NULL OR last_report_received_at_unix<=?)
           AND (sampled_at_unix IS NULL OR sampled_at_unix<?)",
    )
    .bind(now)
    .bind(report.sampled_at_unix)
    .bind(report.cpu_percent)
    .bind(report.logical_cpu_count)
    .bind(report.memory_total_bytes)
    .bind(report.memory_available_bytes)
    .bind(report.uptime_seconds)
    .bind(report.load_average_1m)
    .bind(&report.agent_version)
    .bind(&network_json)
    .bind(&network_json)
    .bind(&network_json.as_ref().map(|_| report.sampled_at_unix))
    .bind(&services_json)
    .bind(&services_json.as_ref().map(|_| report.sampled_at_unix))
    .bind(&services_json.as_ref().map(|_| report.sampled_at_unix))
    .bind(&id)
    .bind(&credential_sha256)
    .bind(now.saturating_sub(REPORT_MIN_INTERVAL_SECONDS))
    .bind(report.sampled_at_unix)
    .execute(&mut *transaction)
    .await;
    match updated {
        Ok(result) if result.rows_affected() == 1 => {
            if let Some(snapshot) = report.network.as_ref() {
                if insert_network_sample(&mut transaction, &id, report.sampled_at_unix, snapshot)
                    .await
                    .is_err()
                    || transaction.commit().await.is_err()
                {
                    return error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "network_sample_storage_unavailable",
                    );
                }
            } else if transaction.commit().await.is_err() {
                return error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "server_node_state_unavailable",
                );
            }
            let mut response = StatusCode::NO_CONTENT.into_response();
            response.headers_mut().insert(
                CACHE_CONTROL,
                axum::http::HeaderValue::from_static("private, no-store"),
            );
            response
        }
        Ok(_) => {
            let _ = transaction.rollback().await;
            let row = sqlx::query(
                "SELECT status,credential_sha256,last_report_received_at_unix,sampled_at_unix
                 FROM platform_server_nodes WHERE id=?",
            )
            .bind(&id)
            .fetch_optional(&state.pool)
            .await;
            match row {
                Ok(Some(row)) if row.get::<String, _>("status") != "active" => {
                    error(StatusCode::UNAUTHORIZED, "reporter_credential_revoked")
                }
                Ok(Some(row)) if row.get::<String, _>("credential_sha256") != credential_sha256 => {
                    error(StatusCode::UNAUTHORIZED, "reporter_credential_invalid")
                }
                Ok(Some(row))
                    if row
                        .get::<Option<i64>, _>("last_report_received_at_unix")
                        .is_some_and(|previous| {
                            now.saturating_sub(previous) < REPORT_MIN_INTERVAL_SECONDS
                        }) =>
                {
                    error(
                        StatusCode::TOO_MANY_REQUESTS,
                        "telemetry_report_too_frequent",
                    )
                }
                Ok(Some(_)) => error(StatusCode::CONFLICT, "telemetry_sample_out_of_order"),
                Ok(None) => error(StatusCode::UNAUTHORIZED, "reporter_credential_invalid"),
                Err(_) => error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "server_node_state_unavailable",
                ),
            }
        }
        Err(_) => error(
            StatusCode::SERVICE_UNAVAILABLE,
            "server_node_state_unavailable",
        ),
    }
}

fn valid_report(report: &TelemetryReport, now: i64) -> bool {
    report.schema_version == 3
        && report
            .network
            .as_ref()
            .map_or(true, crate::network_telemetry::valid_network_snapshot)
        && crate::service_telemetry::valid_service_snapshot(&report.services)
        && report.sampled_at_unix >= now.saturating_sub(REPORT_FRESHNESS_SECONDS)
        && report.sampled_at_unix <= now.saturating_add(MAX_CLOCK_SKEW_SECONDS)
        && report.cpu_percent.is_finite()
        && (0.0..=100.0).contains(&report.cpu_percent)
        && (1..=4096).contains(&report.logical_cpu_count)
        && report.memory_total_bytes > 0
        && report.memory_available_bytes >= 0
        && report.memory_available_bytes <= report.memory_total_bytes
        && report.uptime_seconds >= 0
        && report.load_average_1m.is_finite()
        && (0.0..=1_000_000.0).contains(&report.load_average_1m)
        && !report.agent_version.is_empty()
        && report.agent_version.len() <= MAX_AGENT_VERSION_BYTES
        && report
            .agent_version
            .bytes()
            .all(|byte| byte.is_ascii_graphic())
}

async fn insert_network_sample(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    node_id: &str,
    sampled_at_unix: i64,
    snapshot: &crate::network_telemetry::NetworkSnapshot,
) -> Result<(), sqlx::Error> {
    use crate::network_telemetry::InterfaceCounters;
    fn sum(interfaces: &[InterfaceCounters], value: impl Fn(&InterfaceCounters) -> u64) -> i64 {
        interfaces
            .iter()
            .filter(|interface| !interface.loopback)
            .map(value)
            .fold(0_u64, u64::saturating_add)
            .min(i64::MAX as u64) as i64
    }
    let tcp_states_json =
        serde_json::to_string(&snapshot.tcp_states).unwrap_or_else(|_| "{}".to_owned());
    sqlx::query(
        "INSERT OR IGNORE INTO platform_server_network_samples
         (node_id,sampled_at_unix,rx_bytes,tx_bytes,rx_errors,tx_errors,rx_dropped,tx_dropped,tcp_states_json)
         VALUES (?,?,?,?,?,?,?,?,?)",
    )
    .bind(node_id)
    .bind(sampled_at_unix)
    .bind(sum(&snapshot.interfaces, |item| item.rx_bytes))
    .bind(sum(&snapshot.interfaces, |item| item.tx_bytes))
    .bind(sum(&snapshot.interfaces, |item| item.rx_errors))
    .bind(sum(&snapshot.interfaces, |item| item.tx_errors))
    .bind(sum(&snapshot.interfaces, |item| item.rx_dropped))
    .bind(sum(&snapshot.interfaces, |item| item.tx_dropped))
    .bind(tcp_states_json)
    .execute(&mut **transaction)
    .await?;
    sqlx::query("DELETE FROM platform_server_network_samples WHERE sampled_at_unix<?")
        .bind(sampled_at_unix.saturating_sub(7 * 24 * 60 * 60))
        .execute(&mut **transaction)
        .await?;
    sqlx::query(
        "DELETE FROM platform_server_network_samples
         WHERE rowid IN (
             SELECT rowid FROM platform_server_network_samples WHERE node_id=?
             ORDER BY sampled_at_unix DESC LIMIT -1 OFFSET 20160
         )",
    )
    .bind(node_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub(crate) async fn record_local_network_sample(
    node_id: &str,
    sampled_at_unix: i64,
    snapshot: &crate::network_telemetry::NetworkSnapshot,
) -> Result<(), sqlx::Error> {
    let state = NODE_STATE.get().ok_or(sqlx::Error::PoolTimedOut)?;
    if node_id != crate::server_operation_schema::local_node_id()
        || !crate::network_telemetry::valid_network_snapshot(snapshot)
    {
        return Err(sqlx::Error::Protocol(
            "invalid local network sample".to_owned(),
        ));
    }
    let mut transaction = state.pool.begin().await?;
    insert_network_sample(&mut transaction, node_id, sampled_at_unix, snapshot).await?;
    transaction.commit().await
}

async fn node_network(
    State(state): State<NodeState>,
    Path(id): Path<String>,
    Query(query): Query<NetworkQuery>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = authorize_root(&state, &headers).await {
        return response;
    }
    let seconds = match query.window.as_deref().unwrap_or("1h") {
        "1h" => 60 * 60,
        "6h" => 6 * 60 * 60,
        "24h" => 24 * 60 * 60,
        "7d" => 7 * 24 * 60 * 60,
        _ => return error(StatusCode::BAD_REQUEST, "network_window_invalid"),
    };
    let local_id = crate::server_operation_schema::local_node_id();
    let (latest_at, network) = if id == local_id {
        crate::server_fleet::local_network_snapshot(&id)
            .map(|(sampled_at, snapshot)| (Some(sampled_at), Some(snapshot)))
            .unwrap_or((None, None))
    } else {
        let row = sqlx::query(
            "SELECT status,network_sampled_at_unix,network_json FROM platform_server_nodes WHERE id=?",
        )
        .bind(&id)
        .fetch_optional(&state.pool)
        .await;
        let row = match row {
            Ok(Some(row)) if row.get::<String, _>("status") == "active" => row,
            Ok(Some(_)) => return error(StatusCode::NOT_FOUND, "server_node_unavailable"),
            Ok(None) => return error(StatusCode::NOT_FOUND, "server_node_unavailable"),
            Err(_) => {
                return error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "server_node_state_unavailable",
                );
            }
        };
        let network = row
            .get::<Option<String>, _>("network_json")
            .and_then(|value| {
                serde_json::from_str::<crate::network_telemetry::NetworkSnapshot>(&value).ok()
            })
            .filter(crate::network_telemetry::valid_network_snapshot);
        (row.get("network_sampled_at_unix"), network)
    };
    let since = unix_now().saturating_sub(seconds);
    let bucket_seconds = (seconds / 1008).max(1);
    let rows = sqlx::query(
        "WITH bucketed AS (
             SELECT node_id,sampled_at_unix,CAST(sampled_at_unix / ? AS INTEGER) AS bucket
             FROM platform_server_network_samples WHERE node_id=? AND sampled_at_unix>=?
         ), chosen AS (
             SELECT node_id,bucket,MAX(sampled_at_unix) AS sampled_at_unix
             FROM bucketed GROUP BY node_id,bucket
         )
         SELECT s.sampled_at_unix,s.rx_bytes,s.tx_bytes,s.rx_errors,s.tx_errors,
                s.rx_dropped,s.tx_dropped,s.tcp_states_json
         FROM platform_server_network_samples s
         JOIN chosen c ON c.node_id=s.node_id AND c.sampled_at_unix=s.sampled_at_unix
         ORDER BY s.sampled_at_unix ASC LIMIT 1008",
    )
    .bind(bucket_seconds)
    .bind(&id)
    .bind(since)
    .fetch_all(&state.pool)
    .await;
    match rows {
        Ok(rows) => {
            let samples = rows
                .into_iter()
                .map(|row| NetworkHistorySample {
                    sampled_at_unix: row.get("sampled_at_unix"),
                    rx_bytes: row.get("rx_bytes"),
                    tx_bytes: row.get("tx_bytes"),
                    rx_errors: row.get("rx_errors"),
                    tx_errors: row.get("tx_errors"),
                    rx_dropped: row.get("rx_dropped"),
                    tx_dropped: row.get("tx_dropped"),
                    tcp_states: serde_json::from_str(&row.get::<String, _>("tcp_states_json"))
                        .unwrap_or(serde_json::Value::Null),
                })
                .collect();
            private_json(
                StatusCode::OK,
                NetworkResponse {
                    schema_version: 1,
                    node_id: id,
                    observed_at_unix: latest_at,
                    network,
                    samples,
                },
            )
        }
        Err(_) => error(
            StatusCode::SERVICE_UNAVAILABLE,
            "network_history_unavailable",
        ),
    }
}

fn bearer_credential<'a>(headers: &'a HeaderMap, expected_id: &str) -> Option<&'a str> {
    let value = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    let credential = value.strip_prefix("Bearer ")?;
    let mut parts = credential.split('.');
    let version = parts.next()?;
    let id = parts.next()?;
    let first = parts.next()?;
    let second = parts.next()?;
    if version != "v1"
        || id != expected_id
        || uuid::Uuid::parse_str(id).is_err()
        || first.len() != 32
        || second.len() != 32
        || !first
            .bytes()
            .chain(second.bytes())
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    Some(credential)
}

async fn authorize_root(state: &NodeState, headers: &HeaderMap) -> Result<String, Response> {
    match crate::admin_auth::authorize_root_profile_with_client(
        headers,
        state.service_token.as_deref(),
        &state.authz_endpoint,
        &state.client,
    )
    .await
    {
        Ok(profile_id) => Ok(profile_id),
        Err(failure) => Err(auth_failure(failure)),
    }
}

async fn no_store(request: axum::http::Request<Body>, next: axum::middleware::Next) -> Response {
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        CACHE_CONTROL,
        axum::http::HeaderValue::from_static("private, no-store"),
    );
    response
}

fn state() -> NodeState {
    NODE_STATE.get().cloned().unwrap_or_else(|| NodeState {
        pool: create_pool(),
        authz_endpoint: std::env::var("VAPOR_PLATFORM_IDENTITY_AUTHZ_URL")
            .unwrap_or_else(|_| DEFAULT_IDENTITY_AUTHZ_URL.to_owned()),
        service_token: std::env::var("VAPOR_PLATFORM_IDENTITY_AUTHZ_TOKEN").ok(),
        client: reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(1))
            .timeout(Duration::from_secs(3))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("valid server-node authorization client"),
    })
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

async fn insert_node_event(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    node_id: &str,
    event: &str,
    label: &str,
    role: &str,
    actor: &str,
    now: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO platform_server_node_events
         (node_id,event,label,role,actor_profile_id,at_unix) VALUES (?,?,?,?,?,?)",
    )
    .bind(node_id)
    .bind(event)
    .bind(label)
    .bind(role)
    .bind(actor)
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn node_record(row: sqlx::sqlite::SqliteRow) -> NodeRecord {
    NodeRecord {
        id: row.get("id"),
        slot_id: row
            .get::<Option<String>, _>("slot_id")
            .unwrap_or_else(|| row.get("id")),
        label: row.get("label"),
        role: row.get("role"),
        status: row.get("status"),
        version: row.get("version"),
        created_at_unix: row.get("created_at_unix"),
        created_by_profile_id: row.get("created_by_profile_id"),
        revoked_at_unix: row.get("revoked_at_unix"),
        revoked_by_profile_id: row.get("revoked_by_profile_id"),
        last_report_received_at_unix: row.get("last_report_received_at_unix"),
        sampled_at_unix: row.get("sampled_at_unix"),
        agent_version: row.get("agent_version"),
        executor_enabled: row.get::<i64, _>("executor_enabled") != 0,
        executor_online: row.get::<i64, _>("executor_online") != 0,
    }
}

fn server_slot(row: sqlx::sqlite::SqliteRow) -> ServerSlot {
    ServerSlot {
        id: row.get("id"),
        label: row.get("label"),
        role: row.get("role"),
        status: row.get("status"),
        version: row.get("version"),
        current_node_id: row.get("current_node_id"),
        changed_at_unix: row.get("changed_at_unix"),
    }
}

fn node_event(row: sqlx::sqlite::SqliteRow) -> NodeEvent {
    NodeEvent {
        id: row.get("event_id"),
        node_id: row.get("node_id"),
        event: row.get("event"),
        label: row.get("label"),
        role: row.get("role"),
        actor_profile_id: row.get("actor_profile_id"),
        at_unix: row.get("at_unix"),
    }
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
    private_json(status, serde_json::json!({"error": code}))
}

fn auth_failure(failure: crate::admin_auth::AuthFailure) -> Response {
    let (status, code) = match failure {
        crate::admin_auth::AuthFailure::Unauthorized => {
            (StatusCode::UNAUTHORIZED, "sign_in_required")
        }
        crate::admin_auth::AuthFailure::Forbidden => (StatusCode::FORBIDDEN, "root_required"),
        crate::admin_auth::AuthFailure::Unavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_authorization_unavailable",
        ),
    };
    error(status, code)
}

fn same_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers
        .get(axum::http::header::ORIGIN)
        .and_then(|v| v.to_str().ok())
    else {
        return false;
    };
    let expected = crate::admin_auth::browser_admin_origin();
    origin == expected
}

fn sha256_hex(value: &[u8]) -> String {
    Sha256::digest(value)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{NodeState, router_with_state, sha256_hex};
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use sqlx::sqlite::SqlitePoolOptions;
    use sqlx::{Row, SqlitePool};
    use std::time::Duration;
    use tower::ServiceExt;

    #[test]
    fn telemetry_schema_accepts_only_current_schema_and_rejects_invalid_families() {
        let now = super::unix_now();
        let mut base = serde_json::json!({
            "sampled_at_unix": now,
            "cpu_percent": 1.0,
            "logical_cpu_count": 2,
            "memory_total_bytes": 1024,
            "memory_available_bytes": 512,
            "uptime_seconds": 10,
            "load_average_1m": 0.1,
            "agent_version": "test",
            "network": null
        });
        let service_ids = [
            "homepage",
            "docs",
            "identity",
            "diagnostics",
            "registry",
            "artifact",
            "build-cache",
            "platform",
            "caddy",
        ];
        let service_snapshot = || {
            service_ids
                .iter()
                .map(|service_id| {
                    serde_json::json!({
                        "service_id": service_id,
                        "available": false,
                        "cpu_usage_usec": null,
                        "cpu_millicores": null,
                        "memory_current_bytes": null,
                        "io_read_bytes": null,
                        "io_write_bytes": null
                    })
                })
                .collect::<Vec<_>>()
        };
        base["services"] = serde_json::json!(service_snapshot());
        for version in [1, 2, 4] {
            let mut outdated = base.clone();
            outdated["schema_version"] = serde_json::json!(version);
            let report: super::TelemetryReport = serde_json::from_value(outdated).unwrap();
            assert!(!super::valid_report(&report, now));
        }
        let mut current = base.clone();
        current["schema_version"] = serde_json::json!(3);
        current["services"] = serde_json::json!(service_snapshot());
        let report: super::TelemetryReport = serde_json::from_value(current.clone()).unwrap();
        assert!(super::valid_report(&report, now));

        current["services"][0]["service_id"] = serde_json::json!("../../etc/passwd");
        let report: super::TelemetryReport = serde_json::from_value(current.clone()).unwrap();
        assert!(!super::valid_report(&report, now));

        let mut invalid = current;
        invalid["schema_version"] = serde_json::json!(3);
        invalid["network"] = serde_json::json!({
            "interfaces": (0..17).map(|index| serde_json::json!({
                "name": format!("eth{index}"), "loopback": false,
                "rx_bytes": 0, "rx_packets": 0, "rx_errors": 0, "rx_dropped": 0,
                "tx_bytes": 0, "tx_packets": 0, "tx_errors": 0, "tx_dropped": 0
            })).collect::<Vec<_>>(),
            "tcp_states": serde_json::to_value(crate::network_telemetry::TcpStateCounts::default()).unwrap()
        });
        let report: super::TelemetryReport = serde_json::from_value(invalid).unwrap();
        assert!(!super::valid_report(&report, now));
    }

    async fn root_auth_server() -> (String, tokio::task::JoinHandle<()>) {
        use axum::routing::post;
        let app = axum::Router::new().route(
            "/authorize",
            post(|| async {
                axum::Json(serde_json::json!({
                    "allowed": true,
                    "expires_at_unix": super::unix_now() + 600,
                    "profile_id": "test-root-profile"
                }))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{address}/authorize"), task)
    }

    fn admin_request(method: &str, uri: &str, body: serde_json::Value) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::ORIGIN, "https://vapor.ghf-studios.site")
            .header(
                header::COOKIE,
                format!("vapor_identity_session={}", "a".repeat(40)),
            )
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE platform_server_nodes (
                id TEXT PRIMARY KEY,
                label TEXT NOT NULL,
                role TEXT NOT NULL,
                status TEXT NOT NULL,
                credential_sha256 TEXT NOT NULL,
                executor_credential_sha256 TEXT,
                executor_last_seen_at_unix INTEGER,
                version INTEGER NOT NULL,
                created_at_unix INTEGER NOT NULL,
                created_by_profile_id TEXT NOT NULL,
                revoked_at_unix INTEGER,
                revoked_by_profile_id TEXT,
                last_report_received_at_unix INTEGER,
                sampled_at_unix INTEGER,
                cpu_percent REAL,
                logical_cpu_count INTEGER,
                memory_total_bytes INTEGER,
                memory_available_bytes INTEGER,
                uptime_seconds INTEGER,
                load_average_1m REAL,
                agent_version TEXT,
                network_json TEXT,
                network_sampled_at_unix INTEGER,
                services_json TEXT,
                services_sampled_at_unix INTEGER
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE platform_server_network_samples (
                node_id TEXT NOT NULL,
                sampled_at_unix INTEGER NOT NULL,
                rx_bytes INTEGER NOT NULL,
                tx_bytes INTEGER NOT NULL,
                rx_errors INTEGER NOT NULL,
                tx_errors INTEGER NOT NULL,
                rx_dropped INTEGER NOT NULL,
                tx_dropped INTEGER NOT NULL,
                tcp_states_json TEXT NOT NULL,
                PRIMARY KEY(node_id,sampled_at_unix)
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE platform_server_node_events (
                event_id INTEGER PRIMARY KEY AUTOINCREMENT,
                node_id TEXT NOT NULL,
                event TEXT NOT NULL,
                label TEXT NOT NULL,
                role TEXT NOT NULL,
                actor_profile_id TEXT NOT NULL,
                at_unix INTEGER NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    #[tokio::test]
    async fn logical_slot_migration_preserves_existing_active_node_as_its_incarnation() {
        let pool = test_pool().await;
        let node_id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO platform_server_nodes
             (id,label,role,status,credential_sha256,version,created_at_unix,created_by_profile_id)
             VALUES (?, 'Build node', 'builder', 'active', 'credential-hash', 1, 123, 'owner')",
        )
        .bind(&node_id)
        .execute(&pool)
        .await
        .unwrap();

        super::initialize_slots(&pool).await.unwrap();

        let (slot_id, current_node_id, slot_label, slot_role): (String, String, String, String) =
            sqlx::query_as(
                "SELECT s.id,s.current_node_id,s.label,s.role
                 FROM platform_server_slots s WHERE s.id=?",
            )
            .bind(&node_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        let incarnation_slot_id: String =
            sqlx::query_scalar("SELECT slot_id FROM platform_server_nodes WHERE id=?")
                .bind(&node_id)
                .fetch_one(&pool)
                .await
                .unwrap();

        assert_eq!(slot_id, node_id);
        assert_eq!(current_node_id, node_id);
        assert_eq!(incarnation_slot_id, slot_id);
        assert_eq!(slot_label, "Build node");
        assert_eq!(slot_role, "builder");

        let (local_slot, local_node, local_role): (String, String, String) =
            sqlx::query_as("SELECT id,current_node_id,role FROM platform_server_slots WHERE id=?")
                .bind(crate::server_operation_schema::local_slot_id())
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(local_slot, "core");
        assert_eq!(local_node, crate::server_operation_schema::local_node_id());
        assert_eq!(local_role, "control-plane");
    }

    #[tokio::test]
    async fn logical_slot_migration_keeps_revoked_installation_historical_and_vacant() {
        let pool = test_pool().await;
        let node_id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO platform_server_nodes
             (id,label,role,status,credential_sha256,version,created_at_unix,created_by_profile_id,revoked_at_unix,revoked_by_profile_id)
             VALUES (?, 'Old host', 'backup', 'revoked', 'old-hash', 2, 123, 'owner', 456, 'owner')",
        )
        .bind(&node_id)
        .execute(&pool)
        .await
        .unwrap();

        super::initialize_slots(&pool).await.unwrap();

        let current_node_id: Option<String> =
            sqlx::query_scalar("SELECT current_node_id FROM platform_server_slots WHERE id=?")
                .bind(&node_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        let historical_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM platform_server_nodes WHERE id=? AND status='revoked'",
        )
        .bind(&node_id)
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(current_node_id, None);
        assert_eq!(historical_count, 1);
    }

    #[tokio::test]
    async fn re_enrollment_uses_vacant_slot_and_preserves_old_installation_history() {
        let pool = test_pool().await;
        super::initialize_slots(&pool).await.unwrap();
        let (authz_endpoint, authz_task) = root_auth_server().await;
        let app = router_with_state(NodeState {
            pool: pool.clone(),
            authz_endpoint,
            service_token: Some("a".repeat(64)),
            client: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(1))
                .timeout(Duration::from_secs(3))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap(),
        });

        let created_response = app
            .clone()
            .oneshot(admin_request(
                "POST",
                "/api/admin/server-nodes",
                serde_json::json!({"label":"Worker", "role":"builder"}),
            ))
            .await
            .unwrap();
        let created_status = created_response.status();
        let created_body = axum::body::to_bytes(created_response.into_body(), 4096)
            .await
            .unwrap();
        assert_eq!(
            created_status,
            StatusCode::CREATED,
            "{}",
            String::from_utf8_lossy(&created_body)
        );
        let created: serde_json::Value = serde_json::from_slice(&created_body).unwrap();
        let old_node_id = created["id"].as_str().unwrap().to_owned();
        let slot_id = created["slot_id"].as_str().unwrap().to_owned();
        assert_ne!(old_node_id, slot_id);

        let revoked = app
            .clone()
            .oneshot(admin_request(
                "POST",
                &format!("/api/admin/server-nodes/{old_node_id}/revoke"),
                serde_json::json!({"expected_version":1}),
            ))
            .await
            .unwrap();
        assert_eq!(revoked.status(), StatusCode::OK);

        let replacement_response = app
            .clone()
            .oneshot(admin_request(
                "POST",
                "/api/admin/server-nodes",
                serde_json::json!({"label":"Worker", "role":"builder", "slot_id":slot_id}),
            ))
            .await
            .unwrap();
        assert_eq!(replacement_response.status(), StatusCode::CREATED);
        let replacement: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(replacement_response.into_body(), 4096)
                .await
                .unwrap(),
        )
        .unwrap();
        let new_node_id = replacement["id"].as_str().unwrap();
        assert_ne!(old_node_id, new_node_id);
        assert_eq!(replacement["slot_id"], slot_id);

        let (old_status, new_slot_id): (String, String) =
            sqlx::query_as("SELECT n.status,n.slot_id FROM platform_server_nodes n WHERE n.id=?")
                .bind(old_node_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        let (slot_status, current_node_id): (String, String) =
            sqlx::query_as("SELECT status,current_node_id FROM platform_server_slots WHERE id=?")
                .bind(&slot_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(old_status, "revoked");
        assert_eq!(new_slot_id, slot_id);
        assert_eq!(slot_status, "active");
        assert_eq!(current_node_id, new_node_id);
        authz_task.abort();
    }

    fn telemetry_request_v3(
        node_id: &str,
        credential: &str,
        sampled_at_unix: i64,
        invalid_service_id: bool,
    ) -> Request<Body> {
        let ids = [
            "homepage",
            "docs",
            "identity",
            "diagnostics",
            "registry",
            "artifact",
            "build-cache",
            "platform",
            "caddy",
        ];
        let services = ids
            .iter()
            .enumerate()
            .map(|(index, service_id)| {
                serde_json::json!({
                    "service_id": if invalid_service_id && index == 0 { "not-a-service" } else { service_id },
                    "available": index == 1,
                    "cpu_usage_usec": if index == 1 { serde_json::json!(3000000_u64) } else { serde_json::Value::Null },
                    "cpu_millicores": if index == 1 { serde_json::json!(1000_u32) } else { serde_json::Value::Null },
                    "memory_current_bytes": if index == 1 { serde_json::json!(4096_u64) } else { serde_json::Value::Null },
                    "io_read_bytes": if index == 1 { serde_json::json!(10_u64) } else { serde_json::Value::Null },
                    "io_write_bytes": if index == 1 { serde_json::json!(20_u64) } else { serde_json::Value::Null }
                })
            })
            .collect::<Vec<_>>();
        let body = serde_json::json!({
            "schema_version": 3,
            "sampled_at_unix": sampled_at_unix,
            "cpu_percent": 24.5,
            "logical_cpu_count": 8,
            "memory_total_bytes": 16_000_000_000i64,
            "memory_available_bytes": 8_000_000_000i64,
            "uptime_seconds": 1234,
            "load_average_1m": 1.25,
            "agent_version": "1.0.0",
            "network": {
                "interfaces": [{"name":"eth0","loopback":false,"rx_bytes":300,"rx_packets":3,"rx_errors":0,"rx_dropped":0,"tx_bytes":200,"tx_packets":2,"tx_errors":0,"tx_dropped":0}],
                "tcp_states": {"established":2,"syn_sent":0,"syn_received":0,"fin_wait_1":0,"fin_wait_2":0,"time_wait":1,"close":0,"close_wait":0,"last_ack":0,"listen":1,"closing":0,"other":0}
            },
            "services": services
        });
        Request::builder()
            .method("POST")
            .uri(format!("/api/fleet/v1/nodes/{node_id}/telemetry"))
            .header(header::AUTHORIZATION, format!("Bearer {credential}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    #[tokio::test]
    async fn current_network_report_is_stored_and_available_only_to_root() {
        let pool = test_pool().await;
        let node_id = uuid::Uuid::new_v4().to_string();
        let credential = format!("v1.{node_id}.{}.{}", "a".repeat(32), "b".repeat(32));
        let now = super::unix_now();
        sqlx::query(
            "INSERT INTO platform_server_nodes
             (id,label,role,status,credential_sha256,version,created_at_unix,created_by_profile_id)
             VALUES (?, 'Test node', 'general', 'active', ?, 1, ?, 'test-profile')",
        )
        .bind(&node_id)
        .bind(sha256_hex(credential.as_bytes()))
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
        let (authz_endpoint, authz_task) = root_auth_server().await;
        let app = router_with_state(NodeState {
            pool: pool.clone(),
            authz_endpoint,
            service_token: Some("a".repeat(64)),
            client: reqwest::Client::builder().build().unwrap(),
        });
        let accepted = app
            .clone()
            .oneshot(telemetry_request_v3(&node_id, &credential, now, false))
            .await
            .unwrap();
        assert_eq!(accepted.status(), StatusCode::NO_CONTENT);
        sqlx::query(
            "INSERT INTO platform_server_network_samples
             (node_id,sampled_at_unix,rx_bytes,tx_bytes,rx_errors,tx_errors,rx_dropped,tx_dropped,tcp_states_json)
             VALUES (?,?,200,100,0,0,0,0,'{}')",
        )
        .bind(&node_id)
        .bind(now - 30)
        .execute(&pool)
        .await
        .unwrap();

        let response = app
            .clone()
            .oneshot(admin_request(
                "GET",
                &format!("/api/admin/server-nodes/{node_id}/network?window=1h"),
                serde_json::Value::Null,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "private, no-store"
        );
        let value: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 16 * 1024)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(value["network"]["interfaces"][0]["rx_bytes"], 300);
        assert_eq!(value["samples"][0]["rx_bytes"], 200);
        assert_eq!(value["samples"][1]["rx_bytes"], 300);
        assert!(value["samples"][0].get("remote_address").is_none());

        let invalid_window = app
            .clone()
            .oneshot(admin_request(
                "GET",
                &format!("/api/admin/server-nodes/{node_id}/network?window=30d"),
                serde_json::Value::Null,
            ))
            .await
            .unwrap();
        assert_eq!(invalid_window.status(), StatusCode::BAD_REQUEST);
        authz_task.abort();
    }

    #[tokio::test]
    async fn schema_three_service_snapshot_is_stored_only_after_valid_report() {
        let pool = test_pool().await;
        let node_id = uuid::Uuid::new_v4().to_string();
        let credential = format!("v1.{node_id}.{}.{}", "a".repeat(32), "b".repeat(32));
        let now = super::unix_now();
        sqlx::query(
            "INSERT INTO platform_server_nodes
             (id,label,role,status,credential_sha256,version,created_at_unix,created_by_profile_id)
             VALUES (?, 'Test node', 'general', 'active', ?, 1, ?, 'test-profile')",
        )
        .bind(&node_id)
        .bind(sha256_hex(credential.as_bytes()))
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
        let app = router_with_state(NodeState {
            pool: pool.clone(),
            authz_endpoint: String::new(),
            service_token: None,
            client: reqwest::Client::new(),
        });
        let accepted = app
            .clone()
            .oneshot(telemetry_request_v3(&node_id, &credential, now, false))
            .await
            .unwrap();
        assert_eq!(accepted.status(), StatusCode::NO_CONTENT);
        let (stored_json, stored_at): (String, i64) = sqlx::query_as(
            "SELECT services_json,services_sampled_at_unix FROM platform_server_nodes WHERE id=?",
        )
        .bind(&node_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        let stored: Vec<serde_json::Value> = serde_json::from_str(&stored_json).unwrap();
        assert_eq!(stored_at, now);
        assert_eq!(stored.len(), 9);
        assert_eq!(stored[1]["cpu_millicores"], 1000);

        let rejected = app
            .oneshot(telemetry_request_v3(&node_id, &credential, now + 1, true))
            .await
            .unwrap();
        assert_eq!(rejected.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let preserved: i64 = sqlx::query_scalar(
            "SELECT services_sampled_at_unix FROM platform_server_nodes WHERE id=?",
        )
        .bind(&node_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(preserved, now);
    }

    #[tokio::test]
    async fn telemetry_endpoint_accepts_one_sample_then_enforces_throttle_and_revocation() {
        let pool = test_pool().await;
        let node_id = uuid::Uuid::new_v4().to_string();
        let credential = format!("v1.{node_id}.{}.{}", "a".repeat(32), "b".repeat(32));
        let now = super::unix_now();
        sqlx::query(
            "INSERT INTO platform_server_nodes
             (id,label,role,status,credential_sha256,version,created_at_unix,created_by_profile_id)
             VALUES (?, 'CI worker', 'builder', 'active', ?, 1, ?, 'test-profile')",
        )
        .bind(&node_id)
        .bind(sha256_hex(credential.as_bytes()))
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();

        let app = router_with_state(NodeState {
            pool: pool.clone(),
            authz_endpoint: String::new(),
            service_token: None,
            client: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(1))
                .timeout(Duration::from_secs(3))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap(),
        });

        let invalid_credential = credential.replace('a', "c");
        let rejected = app
            .clone()
            .oneshot(telemetry_request_v3(
                &node_id,
                &invalid_credential,
                now,
                false,
            ))
            .await
            .unwrap();
        assert_eq!(rejected.status(), StatusCode::UNAUTHORIZED);

        let accepted = app
            .clone()
            .oneshot(telemetry_request_v3(&node_id, &credential, now, false))
            .await
            .unwrap();
        assert_eq!(accepted.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            accepted.headers().get(header::CACHE_CONTROL).unwrap(),
            "private, no-store"
        );

        let throttled = app
            .clone()
            .oneshot(telemetry_request_v3(&node_id, &credential, now + 1, false))
            .await
            .unwrap();
        assert_eq!(throttled.status(), StatusCode::TOO_MANY_REQUESTS);
        let stored_sample: i64 =
            sqlx::query("SELECT sampled_at_unix FROM platform_server_nodes WHERE id=?")
                .bind(&node_id)
                .fetch_one(&pool)
                .await
                .unwrap()
                .get("sampled_at_unix");
        assert_eq!(stored_sample, now);
        let stored_cpu: f64 =
            sqlx::query("SELECT cpu_percent FROM platform_server_nodes WHERE id=?")
                .bind(&node_id)
                .fetch_one(&pool)
                .await
                .unwrap()
                .get("cpu_percent");
        assert_eq!(stored_cpu, 24.5);

        sqlx::query(
            "UPDATE platform_server_nodes
             SET last_report_received_at_unix=?,sampled_at_unix=? WHERE id=?",
        )
        .bind(now.saturating_sub(11))
        .bind(now + 1)
        .bind(&node_id)
        .execute(&pool)
        .await
        .unwrap();
        let out_of_order = app
            .clone()
            .oneshot(telemetry_request_v3(&node_id, &credential, now, false))
            .await
            .unwrap();
        assert_eq!(out_of_order.status(), StatusCode::CONFLICT);
        let preserved_sample: i64 =
            sqlx::query("SELECT sampled_at_unix FROM platform_server_nodes WHERE id=?")
                .bind(&node_id)
                .fetch_one(&pool)
                .await
                .unwrap()
                .get("sampled_at_unix");
        assert_eq!(preserved_sample, now + 1);

        sqlx::query("UPDATE platform_server_nodes SET status='revoked' WHERE id=?")
            .bind(&node_id)
            .execute(&pool)
            .await
            .unwrap();
        let revoked = app
            .oneshot(telemetry_request_v3(&node_id, &credential, now + 2, false))
            .await
            .unwrap();
        assert_eq!(revoked.status(), StatusCode::UNAUTHORIZED);
    }
}
