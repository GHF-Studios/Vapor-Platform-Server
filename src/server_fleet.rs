use axum::http::header::CACHE_CONTROL;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DEFAULT_IDENTITY_AUTHZ_URL: &str =
    "http://127.0.0.1:7110/api/identity/v1/internal/authorization/check";
static FLEET_SNAPSHOT: OnceLock<Arc<RwLock<ServerFleetSnapshot>>> = OnceLock::new();
static AUTH_CLIENT: OnceLock<Result<reqwest::Client, reqwest::Error>> = OnceLock::new();

#[derive(Clone, Debug, Serialize)]
struct ServerFleetSnapshot {
    schema_version: u32,
    scope: &'static str,
    observed_at_unix: u64,
    reporting_node_count: usize,
    nodes: Vec<ServerNode>,
}

#[derive(Clone, Debug, Serialize)]
struct ServerNode {
    id: String,
    label: String,
    role: &'static str,
    state: &'static str,
    metrics_state: &'static str,
    observed_at_unix: u64,
    cpu_percent: Option<f64>,
    logical_cpu_count: Option<usize>,
    memory_total_bytes: Option<u64>,
    memory_available_bytes: Option<u64>,
    uptime_seconds: Option<u64>,
    load_average_1m: Option<f64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CpuTimes {
    total: u64,
    idle: u64,
}

#[derive(Default)]
struct Sampler {
    previous_cpu: Option<CpuTimes>,
}

pub fn initialize() {
    let mut sampler = Sampler::default();
    let initial = sampler.snapshot();
    let shared = Arc::new(RwLock::new(initial));
    let _ = FLEET_SNAPSHOT.set(shared.clone());
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        interval.tick().await;
        loop {
            interval.tick().await;
            let snapshot = sampler.snapshot();
            if let Ok(mut current) = shared.write() {
                *current = snapshot;
            }
        }
    });
}

impl Sampler {
    fn snapshot(&mut self) -> ServerFleetSnapshot {
        let cpu = read_cpu_times();
        let cpu_percent = match (self.previous_cpu, cpu) {
            (Some(previous), Some(current)) => cpu_usage_percent(
                (previous.total, previous.idle),
                (current.total, current.idle),
            ),
            _ => None,
        };
        if cpu.is_some() {
            self.previous_cpu = cpu;
        }

        let memory = read_meminfo();
        let uptime = read_uptime();
        let load_average_1m = read_load_average();
        let observed_at_unix = unix_now();
        let has_metrics = cpu.is_some() || memory.is_some() || uptime.is_some();
        let node = ServerNode {
            id: node_id(),
            label: node_label(),
            role: "control-plane",
            state: if has_metrics { "online" } else { "unavailable" },
            metrics_state: if !has_metrics {
                "unavailable"
            } else if cpu_percent.is_some() && memory.is_some() && uptime.is_some() {
                "complete"
            } else {
                "partial"
            },
            observed_at_unix,
            cpu_percent,
            logical_cpu_count: std::thread::available_parallelism().ok().map(usize::from),
            memory_total_bytes: memory.map(|values| values.0),
            memory_available_bytes: memory.map(|values| values.1),
            uptime_seconds: uptime,
            load_average_1m,
        };
        ServerFleetSnapshot {
            schema_version: 1,
            scope: "local-platform-host-only",
            observed_at_unix,
            reporting_node_count: usize::from(has_metrics),
            nodes: vec![node],
        }
    }
}

pub async fn admin_snapshot(headers: HeaderMap) -> Response {
    let endpoint = std::env::var("VAPOR_PLATFORM_IDENTITY_AUTHZ_URL")
        .unwrap_or_else(|_| DEFAULT_IDENTITY_AUTHZ_URL.to_owned());
    let service_token = std::env::var("VAPOR_PLATFORM_IDENTITY_AUTHZ_TOKEN").ok();
    let client = match AUTH_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(1))
            .timeout(Duration::from_secs(3))
            .redirect(reqwest::redirect::Policy::none())
            .build()
    }) {
        Ok(client) => client,
        Err(_) => return auth_failure(crate::admin_auth::AuthFailure::Unavailable),
    };
    if let Err(failure) = crate::admin_auth::authorize_root_with_client(
        &headers,
        service_token.as_deref(),
        &endpoint,
        client,
    )
    .await
    {
        return auth_failure(failure);
    }

    let snapshot = FLEET_SNAPSHOT
        .get()
        .and_then(|snapshot| snapshot.read().ok().map(|value| value.clone()));
    match snapshot {
        Some(snapshot) => {
            let mut response = (StatusCode::OK, Json(snapshot)).into_response();
            response.headers_mut().insert(
                CACHE_CONTROL,
                axum::http::HeaderValue::from_static("no-store"),
            );
            response
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error":"server_fleet_not_initialized"})),
        )
            .into_response(),
    }
}

fn auth_failure(failure: crate::admin_auth::AuthFailure) -> Response {
    let mut response = (
        failure.status(),
        Json(serde_json::json!({"error":match failure {
            crate::admin_auth::AuthFailure::Unauthorized => "sign_in_required",
            crate::admin_auth::AuthFailure::Forbidden => "root_required",
            crate::admin_auth::AuthFailure::Unavailable => "identity_authorization_unavailable",
        }})),
    )
        .into_response();
    response.headers_mut().insert(
        CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    response
}

fn read_cpu_times() -> Option<CpuTimes> {
    std::fs::read_to_string("/proc/stat")
        .ok()
        .and_then(|contents| parse_cpu_times(contents.lines().next()?))
}

fn read_meminfo() -> Option<(u64, u64)> {
    std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|contents| parse_meminfo(&contents))
}

fn read_uptime() -> Option<u64> {
    std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|contents| parse_uptime(&contents))
}

fn read_load_average() -> Option<f64> {
    let contents = std::fs::read_to_string("/proc/loadavg").ok()?;
    let value = contents.split_whitespace().next()?.parse::<f64>().ok()?;
    value.is_finite().then_some(value)
}

fn node_id() -> String {
    let machine_id = std::fs::read_to_string("/etc/machine-id")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "local-development-node".to_owned());
    let digest = Sha256::digest(machine_id.as_bytes());
    let short_hash = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("node-{short_hash}")
}

fn node_label() -> String {
    std::env::var("VAPOR_PLATFORM_NODE_LABEL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        })
        .unwrap_or_else(|| "Local development host".to_owned())
}

fn parse_cpu_times(line: &str) -> Option<CpuTimes> {
    let mut fields = line.split_whitespace();
    if fields.next()? != "cpu" {
        return None;
    }
    let counters = fields
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if counters.len() < 4 {
        return None;
    }
    let total = counters.iter().copied().fold(0_u64, u64::saturating_add);
    let idle = counters[3].saturating_add(counters.get(4).copied().unwrap_or_default());
    Some(CpuTimes { total, idle })
}

fn cpu_usage_percent(previous: (u64, u64), current: (u64, u64)) -> Option<f64> {
    let total_delta = current.0.checked_sub(previous.0)?;
    let idle_delta = current.1.checked_sub(previous.1)?;
    if total_delta == 0 || idle_delta > total_delta {
        return None;
    }
    let busy_delta = total_delta - idle_delta;
    Some((busy_delta as f64 / total_delta as f64 * 100.0).clamp(0.0, 100.0))
}

fn parse_meminfo(contents: &str) -> Option<(u64, u64)> {
    let mut total = None;
    let mut available = None;
    for line in contents.lines() {
        let (key, value) = line.split_once(':')?;
        if !matches!(key, "MemTotal" | "MemAvailable") {
            continue;
        }
        let mut parts = value.split_whitespace();
        let amount = parts.next()?.parse::<u64>().ok()?;
        if parts.next()? != "kB" {
            return None;
        }
        match key {
            "MemTotal" => total = amount.checked_mul(1024),
            "MemAvailable" => available = amount.checked_mul(1024),
            _ => {}
        }
    }
    Some((total?, available?))
}

fn parse_uptime(contents: &str) -> Option<u64> {
    let seconds = contents.split_whitespace().next()?.parse::<f64>().ok()?;
    (seconds.is_finite() && seconds >= 0.0).then_some(seconds as u64)
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{admin_snapshot, cpu_usage_percent, parse_cpu_times, parse_meminfo, parse_uptime};
    use axum::http::{HeaderMap, StatusCode};

    #[test]
    fn parses_linux_cpu_counters_and_excludes_idle_time() {
        let parsed = parse_cpu_times("cpu 100 2 30 800 20 0 0 0").unwrap();
        assert_eq!(parsed.total, 952);
        assert_eq!(parsed.idle, 820);
    }

    #[test]
    fn computes_cpu_busy_percentage_from_counter_deltas() {
        assert_eq!(cpu_usage_percent((100, 70), (200, 140)), Some(30.0));
    }

    #[test]
    fn cpu_percentage_is_unknown_before_a_sample_and_on_counter_reset() {
        assert_eq!(cpu_usage_percent((200, 140), (100, 70)), None);
        assert_eq!(cpu_usage_percent((100, 70), (100, 70)), None);
    }

    #[test]
    fn parses_memory_and_uptime_without_treating_missing_values_as_zero() {
        assert_eq!(
            parse_meminfo("MemTotal: 1000 kB\nMemAvailable: 250 kB\n"),
            Some((1_024_000, 256_000))
        );
        assert_eq!(parse_meminfo("MemTotal: 1000 kB\n"), None);
        assert_eq!(parse_uptime("12345.67 2.0"), Some(12_345));
        assert_eq!(parse_uptime("invalid"), None);
    }

    #[tokio::test]
    async fn server_inventory_requires_a_root_identity_session() {
        let response = admin_snapshot(HeaderMap::new()).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response
                .headers()
                .get("cache-control")
                .and_then(|value| value.to_str().ok()),
            Some("no-store")
        );
    }
}
