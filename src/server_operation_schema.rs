use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ServerAction {
    RestartService { service: ServiceId },
    RebootHost,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ServiceId {
    Platform,
    Homepage,
    Docs,
    Identity,
    Diagnostics,
    Registry,
    Artifact,
    BuildCache,
    Caddy,
}

impl ServerAction {
    pub(crate) fn parse_json(value: &serde_json::Value) -> Result<Self, &'static str> {
        let object = value.as_object().ok_or("unsupported server action")?;
        let kind = object
            .get("kind")
            .and_then(serde_json::Value::as_str)
            .ok_or("unsupported server action")?;
        let expected_fields = match kind {
            "restart_service" => &["kind", "service"][..],
            "reboot_host" => &["kind"][..],
            _ => return Err("unsupported server action"),
        };
        if object.len() != expected_fields.len()
            || object
                .keys()
                .any(|field| !expected_fields.contains(&field.as_str()))
        {
            return Err("unsupported server action");
        }
        serde_json::from_value(value.clone()).map_err(|_| "unsupported server action")
    }

    pub(crate) fn systemd_unit(&self) -> Option<&'static str> {
        match self {
            Self::RestartService { service } => Some(service.systemd_unit()),
            Self::RebootHost => None,
        }
    }
}

impl ServiceId {
    pub(crate) fn systemd_unit(&self) -> &'static str {
        match self {
            Self::Platform => "vapor-platform-server.service",
            Self::Homepage => "vapor-homepage.service",
            Self::Docs => "vapor-docs.service",
            Self::Identity => "vapor-identity.service",
            Self::Diagnostics => "vapor-diagnostics.service",
            Self::Registry => "vapor-registry.service",
            Self::Artifact => "vapor-artifact.service",
            Self::BuildCache => "vapor-build-cache.service",
            Self::Caddy => "caddy.service",
        }
    }
}

pub(crate) fn local_node_id() -> String {
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

#[cfg(test)]
mod tests {
    use super::{ServerAction, ServiceId};
    use serde_json::json;

    #[test]
    fn accepts_only_the_fixed_restart_service_vocabulary() {
        assert_eq!(
            ServerAction::parse_json(&json!({"kind":"restart_service","service":"docs"})),
            Ok(ServerAction::RestartService {
                service: ServiceId::Docs
            })
        );
    }

    #[test]
    fn rejects_unknown_service_and_command_fields() {
        for value in [
            json!({"kind":"restart_service","service":"sshd"}),
            json!({"kind":"restart_service","service":"docs","command":"id"}),
            json!({"kind":"run_command","command":"id"}),
        ] {
            assert!(ServerAction::parse_json(&value).is_err(), "{value}");
        }
    }

    #[test]
    fn accepts_reboot_without_arbitrary_parameters() {
        assert_eq!(
            ServerAction::parse_json(&json!({"kind":"reboot_host"})),
            Ok(ServerAction::RebootHost)
        );
        assert!(ServerAction::parse_json(&json!({
            "kind":"reboot_host",
            "delay_seconds":0
        }))
        .is_err());
    }

    #[test]
    fn service_ids_map_only_to_fixed_systemd_units() {
        assert_eq!(
            ServerAction::RestartService {
                service: ServiceId::Caddy
            }
            .systemd_unit(),
            Some("caddy.service")
        );
        assert_eq!(
            ServerAction::RestartService {
                service: ServiceId::Platform
            }
            .systemd_unit(),
            Some("vapor-platform-server.service")
        );
    }
}
