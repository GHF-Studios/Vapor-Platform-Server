#[cfg(target_os = "linux")]
use reqwest::Url;
#[cfg(target_os = "linux")]
use serde::{Deserialize, Serialize};
#[cfg(target_os = "linux")]
use std::path::PathBuf;
#[cfg(target_os = "linux")]
use std::time::Duration;

#[cfg(target_os = "linux")]
const REPORT_INTERVAL_SECONDS: u64 = 30;
#[cfg(target_os = "linux")]
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(target_os = "linux")]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    platform_origin: String,
    node_id: String,
    reporter_credential: String,
    #[serde(default)]
    executor_credential: Option<String>,
}

#[cfg(target_os = "linux")]
#[derive(Serialize)]
struct TelemetryReport {
    schema_version: u32,
    sampled_at_unix: i64,
    cpu_percent: f64,
    logical_cpu_count: i64,
    memory_total_bytes: i64,
    memory_available_bytes: i64,
    uptime_seconds: i64,
    load_average_1m: f64,
    agent_version: String,
}

#[cfg(target_os = "linux")]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoteClaim {
    operation: Option<RemoteOperation>,
}

#[cfg(target_os = "linux")]
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RemoteOperation {
    id: String,
    schema_version: u32,
    target_node_id: String,
    action: RemoteAction,
    expires_at_unix: i64,
}

#[cfg(target_os = "linux")]
#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum RemoteAction {
    RestartService { service: RemoteServiceId },
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum RemoteServiceId {
    Docs,
    Homepage,
    Identity,
    Diagnostics,
    Registry,
    Artifact,
    BuildCache,
}

#[cfg(target_os = "linux")]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoteResult {
    state: String,
    detail: String,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy)]
struct CpuTimes {
    total: u64,
    idle: u64,
}

#[cfg(target_os = "linux")]
struct Sampler {
    previous_cpu: Option<CpuTimes>,
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("vapor-node-agent: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    #[cfg(not(target_os = "linux"))]
    return Err("the node reporter currently supports Linux only".to_owned());

    #[cfg(target_os = "linux")]
    {
        let (config_path, once) = parse_args()?;
        let config = read_config(&config_path)?;
        validate_config(&config)?;
        let endpoint = Url::parse(&format!(
            "{}/api/fleet/v1/nodes/{}/telemetry",
            config.platform_origin.trim_end_matches('/'),
            config.node_id
        ))
        .map_err(|_| "invalid Platform telemetry URL".to_owned())?;
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| "could not create HTTPS client".to_owned())?;
        if let Some(credential) = &config.executor_credential {
            validate_credential(&config.node_id, credential, "executor")?;
        }
        if !once {
            if let Some(executor_credential) = config.executor_credential.clone() {
                let operation_client = client.clone();
                let operation_origin = config.platform_origin.clone();
                let operation_node_id = config.node_id.clone();
                tokio::spawn(async move {
                    poll_operations(
                        operation_client,
                        operation_origin,
                        operation_node_id,
                        executor_credential,
                    )
                    .await;
                });
            }
        }
        let mut sampler = Sampler {
            previous_cpu: read_cpu_times(),
        };
        tokio::time::sleep(Duration::from_secs(1)).await;
        if once {
            if let Some(report) = sample(&mut sampler)? {
                send_report(&client, &endpoint, &config.reporter_credential, report).await?;
                return Ok(());
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
            let report = sample(&mut sampler)?.ok_or("CPU baseline is not available")?;
            send_report(&client, &endpoint, &config.reporter_credential, report).await?;
            return Ok(());
        }

        loop {
            if let Some(report) = sample(&mut sampler)? {
                match send_report(&client, &endpoint, &config.reporter_credential, report).await {
                    Ok(()) => eprintln!("vapor-node-agent: telemetry accepted"),
                    Err(_) => eprintln!(
                        "vapor-node-agent: telemetry delivery failed; will retry next interval"
                    ),
                }
            }
            tokio::time::sleep(Duration::from_secs(REPORT_INTERVAL_SECONDS)).await;
        }
    }
}

#[cfg(target_os = "linux")]
fn parse_args() -> Result<(PathBuf, bool), String> {
    let mut arguments = std::env::args().skip(1);
    let mut config = None;
    let mut once = false;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--config" => {
                config = Some(PathBuf::from(
                    arguments.next().ok_or("--config requires a path")?,
                ));
            }
            "--once" => once = true,
            _ => return Err("usage: vapor-node-agent --config PATH [--once]".to_owned()),
        }
    }
    Ok((
        config.ok_or("usage: vapor-node-agent --config PATH [--once]".to_owned())?,
        once,
    ))
}

#[cfg(target_os = "linux")]
fn read_config(path: &std::path::Path) -> Result<Config, String> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let metadata = std::fs::symlink_metadata(path)
            .map_err(|_| "could not read node-agent config".to_owned())?;
        let effective_group = unsafe { libc::getegid() };
        let group_mode = metadata.permissions().mode() & 0o070;
        if !metadata.file_type().is_file()
            || metadata.uid() != 0
            || metadata.gid() != effective_group
            || metadata.permissions().mode() & 0o007 != 0
            || group_mode != 0o040
        {
            return Err("node-agent config must be a root-owned regular file readable only by its dedicated service group".to_owned());
        }
    }
    let contents =
        std::fs::read_to_string(path).map_err(|_| "could not read node-agent config".to_owned())?;
    toml::from_str(&contents)
        .map_err(|_| "node-agent config has invalid TOML or unknown fields".to_owned())
}

#[cfg(target_os = "linux")]
fn validate_config(config: &Config) -> Result<(), String> {
    let origin = Url::parse(&config.platform_origin)
        .map_err(|_| "Platform origin must be a bare HTTPS origin".to_owned())?;
    if origin.scheme() != "https"
        || origin.host_str().is_none()
        || !origin.username().is_empty()
        || origin.password().is_some()
        || !matches!(origin.path(), "" | "/")
        || origin.query().is_some()
        || origin.fragment().is_some()
    {
        return Err("Platform origin must be a bare HTTPS origin with no credentials, path, query, or fragment".to_owned());
    }
    if uuid::Uuid::parse_str(&config.node_id).is_err() {
        return Err("node_id must be a UUID".to_owned());
    }
    validate_credential(&config.node_id, &config.reporter_credential, "reporter")
}

#[cfg(target_os = "linux")]
fn validate_credential(node_id: &str, credential: &str, name: &str) -> Result<(), String> {
    let prefix = format!("v1.{node_id}.");
    let Some(secret) = credential.strip_prefix(&prefix) else {
        return Err(format!("{name} credential does not belong to this node"));
    };
    let mut sections = secret.split('.');
    let first = sections.next().unwrap_or_default();
    let second = sections.next().unwrap_or_default();
    if sections.next().is_some()
        || first.len() != 32
        || second.len() != 32
        || !first
            .bytes()
            .chain(second.bytes())
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(format!("{name} credential has an invalid format"));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
async fn poll_operations(
    client: reqwest::Client,
    platform_origin: String,
    node_id: String,
    credential: String,
) {
    let endpoint = format!(
        "{}/api/fleet/v1/nodes/{node_id}/operations/claim",
        platform_origin.trim_end_matches('/')
    );
    let mut interval = tokio::time::interval(Duration::from_secs(3));
    interval.tick().await;
    loop {
        interval.tick().await;
        let response = match client
            .get(&endpoint)
            .bearer_auth(&credential)
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => response,
            _ => continue,
        };
        let claim: RemoteClaim = match response.json().await {
            Ok(claim) => claim,
            Err(_) => continue,
        };
        let Some(operation) = claim.operation else {
            continue;
        };
        if operation.target_node_id != node_id
            || operation.schema_version != 1
            || uuid::Uuid::parse_str(&operation.id).is_err()
            || operation.expires_at_unix <= unix_now()
            || operation.expires_at_unix > unix_now().saturating_add(900)
        {
            let _ = send_remote_result(
                &client,
                &platform_origin,
                &node_id,
                &credential,
                &operation.id,
                "failed",
                "claimed operation identity or expiry is invalid",
            )
            .await;
            continue;
        }
        let payload = serde_json::json!({
            "schema_version": operation.schema_version,
            "operation_id": operation.id,
            "target_node_id": operation.target_node_id,
            "action": operation.action,
            "expires_at_unix": operation.expires_at_unix,
        });
        let helper = match execute_remote_operation(payload).await {
            Ok(result) => result,
            Err(_) => RemoteResult {
                state: "outcome_unknown".to_owned(),
                detail: "executor result was not conclusive; inspect the host before retrying".to_owned(),
            },
        };
        let _ = send_remote_result(
            &client,
            &platform_origin,
            &node_id,
            &credential,
            &operation.id,
            &helper.state,
            &helper.detail,
        )
        .await;
    }
}

#[cfg(target_os = "linux")]
async fn execute_remote_operation(payload: serde_json::Value) -> Result<RemoteResult, String> {
    use tokio::io::AsyncWriteExt;
    use tokio::process::Command;

    let bytes = serde_json::to_vec(&payload).map_err(|_| "could not encode action")?;
    if bytes.len() > 2048 {
        return Err("action payload is too large".to_owned());
    }
    let mut child = Command::new("/usr/bin/sudo")
        .args(["-n", "/usr/local/libexec/vapor-node-ops"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| "could not start the restricted operation helper")?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(&bytes)
            .await
            .map_err(|_| "could not deliver the typed operation")?;
    }
    let output = tokio::time::timeout(Duration::from_secs(90), child.wait_with_output())
        .await
        .map_err(|_| "restricted operation timed out")?
        .map_err(|_| "restricted operation could not be observed")?;
    if !output.status.success() {
        return Err("restricted operation helper rejected or failed the action".to_owned());
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|_| "restricted operation helper returned invalid output".to_owned())
}

#[cfg(target_os = "linux")]
async fn send_remote_result(
    client: &reqwest::Client,
    platform_origin: &str,
    node_id: &str,
    credential: &str,
    operation_id: &str,
    state: &str,
    detail: &str,
) -> Result<(), String> {
    let endpoint = format!(
        "{}/api/fleet/v1/nodes/{node_id}/operations/{operation_id}/result",
        platform_origin.trim_end_matches('/')
    );
    let response = client
        .post(endpoint)
        .bearer_auth(credential)
        .json(&serde_json::json!({"state":state,"detail":detail}))
        .send()
        .await
        .map_err(|_| "could not report the operation result")?;
    if response.status().as_u16() != 204 {
        return Err("Platform did not accept the operation result".to_owned());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or_default()
}

#[cfg(target_os = "linux")]
fn sample(sampler: &mut Sampler) -> Result<Option<TelemetryReport>, String> {
    let current_cpu = read_cpu_times().ok_or("could not read CPU counters")?;
    let cpu_percent = sampler
        .previous_cpu
        .and_then(|previous| cpu_usage(previous, current_cpu));
    sampler.previous_cpu = Some(current_cpu);
    let Some(cpu_percent) = cpu_percent else {
        return Ok(None);
    };
    let (memory_total_bytes, memory_available_bytes) = read_memory()?;
    let uptime_seconds = read_uptime()?;
    let load_average_1m = read_load_average()?;
    let logical_cpu_count = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1) as i64;
    let sampled_at_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "system clock is before the Unix epoch")?
        .as_secs() as i64;
    Ok(Some(TelemetryReport {
        schema_version: 1,
        sampled_at_unix,
        cpu_percent,
        logical_cpu_count,
        memory_total_bytes,
        memory_available_bytes,
        uptime_seconds,
        load_average_1m,
        agent_version: VERSION.to_owned(),
    }))
}

#[cfg(target_os = "linux")]
async fn send_report(
    client: &reqwest::Client,
    endpoint: &Url,
    credential: &str,
    report: TelemetryReport,
) -> Result<(), String> {
    let response = client
        .post(endpoint.clone())
        .bearer_auth(credential)
        .json(&report)
        .send()
        .await
        .map_err(|_| "telemetry connection failed".to_owned())?;
    if response.status().is_success() {
        return Ok(());
    }
    Err(format!(
        "telemetry rejected with HTTP {}",
        response.status().as_u16()
    ))
}

#[cfg(target_os = "linux")]
fn read_cpu_times() -> Option<CpuTimes> {
    let contents = std::fs::read_to_string("/proc/stat").ok()?;
    let mut fields = contents.lines().next()?.split_whitespace();
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
    Some(CpuTimes {
        total: counters.iter().copied().fold(0_u64, u64::saturating_add),
        idle: counters[3].saturating_add(counters.get(4).copied().unwrap_or_default()),
    })
}

#[cfg(target_os = "linux")]
fn cpu_usage(previous: CpuTimes, current: CpuTimes) -> Option<f64> {
    let total = current.total.checked_sub(previous.total)?;
    let idle = current.idle.checked_sub(previous.idle)?;
    if total == 0 || idle > total {
        return None;
    }
    Some(((total - idle) as f64 / total as f64 * 100.0).clamp(0.0, 100.0))
}

#[cfg(target_os = "linux")]
fn read_memory() -> Result<(i64, i64), String> {
    let contents =
        std::fs::read_to_string("/proc/meminfo").map_err(|_| "could not read memory counters")?;
    let mut total = None;
    let mut available = None;
    for line in contents.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let Some(amount) = value
            .split_whitespace()
            .next()
            .and_then(|value| value.parse::<i64>().ok())
        else {
            continue;
        };
        match key {
            "MemTotal" => total = amount.checked_mul(1024),
            "MemAvailable" => available = amount.checked_mul(1024),
            _ => {}
        }
    }
    let total = total.ok_or("memory total is unavailable")?;
    let available = available.ok_or("available memory is unavailable")?;
    if total <= 0 || available < 0 || available > total {
        return Err("memory counters are invalid".to_owned());
    }
    Ok((total, available))
}

#[cfg(target_os = "linux")]
fn read_uptime() -> Result<i64, String> {
    let contents = std::fs::read_to_string("/proc/uptime").map_err(|_| "could not read uptime")?;
    let seconds = contents
        .split_whitespace()
        .next()
        .and_then(|value| value.parse::<f64>().ok())
        .ok_or("uptime is invalid")?;
    if !seconds.is_finite() || seconds < 0.0 || seconds > i64::MAX as f64 {
        return Err("uptime is invalid".to_owned());
    }
    Ok(seconds as i64)
}

#[cfg(target_os = "linux")]
fn read_load_average() -> Result<f64, String> {
    let contents =
        std::fs::read_to_string("/proc/loadavg").map_err(|_| "could not read load average")?;
    let load = contents
        .split_whitespace()
        .next()
        .and_then(|value| value.parse::<f64>().ok())
        .ok_or("load average is invalid")?;
    if !load.is_finite() || load < 0.0 {
        return Err("load average is invalid".to_owned());
    }
    Ok(load)
}
