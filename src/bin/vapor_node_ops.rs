#[cfg(target_os = "linux")]
use serde::Deserialize;

#[cfg(target_os = "linux")]
use std::io::{Read, Write};

#[cfg(target_os = "linux")]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Operation {
    schema_version: u32,
    operation_id: String,
    target_node_id: String,
    expires_at_unix: i64,
    action: Action,
}

#[cfg(target_os = "linux")]
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Action {
    RestartService { service: ServiceId },
}

#[cfg(target_os = "linux")]
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum ServiceId {
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
struct AgentConfig {
    node_id: String,
}

fn main() {
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("vapor-node-ops is supported only on Linux");
        std::process::exit(2);
    }

    #[cfg(target_os = "linux")]
    if let Err(error) = run() {
        eprintln!("vapor-node-ops: {error}");
        std::process::exit(2);
    }
}

#[cfg(target_os = "linux")]
fn run() -> Result<(), &'static str> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    if std::env::args_os().len() != 1 {
        return Err("arguments are not accepted");
    }
    let config_path = "/etc/vapor/node-agent.toml";
    let metadata = std::fs::symlink_metadata(config_path)
        .map_err(|_| "could not read protected node config")?;
    if !metadata.file_type().is_file()
        || metadata.uid() != 0
        || metadata.permissions().mode() & 0o022 != 0
    {
        return Err("node config ownership or permissions are unsafe");
    }
    let config_text =
        std::fs::read_to_string(config_path).map_err(|_| "could not read protected node config")?;
    let config: AgentConfig =
        toml::from_str(&config_text).map_err(|_| "node config has invalid TOML")?;

    let mut bytes = Vec::new();
    std::io::stdin()
        .take(2049)
        .read_to_end(&mut bytes)
        .map_err(|_| "could not read operation")?;
    if bytes.is_empty() || bytes.len() > 2048 {
        return Err("operation payload size is invalid");
    }
    let operation = parse_operation(&bytes, &config.node_id, unix_now())?;
    let Action::RestartService { service } = operation.action;
    let unit = service.unit();
    let result = std::process::Command::new("/usr/bin/systemctl")
        .args(["restart", unit])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    let (state, detail) = match result {
        Ok(status) if status.success() => ("succeeded", "approved service restart completed"),
        _ => (
            "failed",
            "approved service restart failed; inspect the host",
        ),
    };
    serde_json::to_writer(
        std::io::stdout().lock(),
        &serde_json::json!({"state": state, "detail": detail}),
    )
    .map_err(|_| "could not encode operation result")?;
    std::io::stdout()
        .lock()
        .flush()
        .map_err(|_| "could not flush operation result")?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn parse_operation(
    bytes: &[u8],
    expected_node_id: &str,
    now: i64,
) -> Result<Operation, &'static str> {
    let operation: Operation =
        serde_json::from_slice(bytes).map_err(|_| "operation payload is invalid")?;
    if operation.schema_version != 1
        || uuid::Uuid::parse_str(&operation.operation_id).is_err()
        || operation.target_node_id != expected_node_id
        || operation.expires_at_unix <= now
        || operation.expires_at_unix > now.saturating_add(900)
    {
        return Err("operation identity or expiry is invalid");
    }
    Ok(operation)
}

#[cfg(target_os = "linux")]
impl ServiceId {
    fn unit(&self) -> &'static str {
        match self {
            Self::Docs => "vapor-docs.service",
            Self::Homepage => "vapor-homepage.service",
            Self::Identity => "vapor-identity.service",
            Self::Diagnostics => "vapor-diagnostics.service",
            Self::Registry => "vapor-registry.service",
            Self::Artifact => "vapor-artifact.service",
            Self::BuildCache => "vapor-build-cache.service",
        }
    }
}

#[cfg(target_os = "linux")]
fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or_default()
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::parse_operation;

    const NODE: &str = "11111111-1111-4111-8111-111111111111";
    const OPERATION: &str = "22222222-2222-4222-8222-222222222222";

    #[test]
    fn accepts_a_live_allowlisted_typed_restart_for_the_configured_node() {
        let input = format!(
            r#"{{"schema_version":1,"operation_id":"{OPERATION}","target_node_id":"{NODE}","expires_at_unix":1200,"action":{{"kind":"restart_service","service":"docs"}}}}"#
        );
        assert!(parse_operation(input.as_bytes(), NODE, 1000).is_ok());
    }

    #[test]
    fn rejects_control_plane_services_reboot_unknown_fields_wrong_node_and_expiry() {
        let cases = [
            r#"{"schema_version":1,"operation_id":"22222222-2222-4222-8222-222222222222","target_node_id":"11111111-1111-4111-8111-111111111111","expires_at_unix":1200,"action":{"kind":"restart_service","service":"caddy"}}"#,
            r#"{"schema_version":1,"operation_id":"22222222-2222-4222-8222-222222222222","target_node_id":"11111111-1111-4111-8111-111111111111","expires_at_unix":1200,"action":{"kind":"reboot_host","confirmation":"REBOOT host"}}"#,
            r#"{"schema_version":1,"operation_id":"22222222-2222-4222-8222-222222222222","target_node_id":"11111111-1111-4111-8111-111111111111","expires_at_unix":1200,"action":{"kind":"restart_service","service":"docs","command":"id"}}"#,
            r#"{"schema_version":1,"operation_id":"22222222-2222-4222-8222-222222222222","target_node_id":"33333333-3333-4333-8333-333333333333","expires_at_unix":1200,"action":{"kind":"restart_service","service":"docs"}}"#,
            r#"{"schema_version":1,"operation_id":"22222222-2222-4222-8222-222222222222","target_node_id":"11111111-1111-4111-8111-111111111111","expires_at_unix":999,"action":{"kind":"restart_service","service":"docs"}}"#,
        ];
        for input in cases {
            assert!(
                parse_operation(input.as_bytes(), NODE, 1000).is_err(),
                "{input}"
            );
        }
    }
}
