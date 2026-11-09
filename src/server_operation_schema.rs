use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ServerAction {
    RestartService { service: ServiceId },
    RebootHost { confirmation: String },
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
            "reboot_host" => &["kind", "confirmation"][..],
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

pub(crate) fn local_slot_id() -> String {
    std::env::var("VAPOR_PLATFORM_SERVER_SLOT_ID")
        .ok()
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 80
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
        .unwrap_or_else(|| "core".to_owned())
}

pub(crate) fn current_boot_id() -> Option<String> {
    std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| uuid::Uuid::parse_str(value).is_ok())
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
            ServerAction::parse_json(
                &json!({"kind":"reboot_host","confirmation":"REBOOT node-abcd"})
            ),
            Ok(ServerAction::RebootHost {
                confirmation: "REBOOT node-abcd".to_owned()
            })
        );
        assert!(ServerAction::parse_json(&json!({
            "kind":"reboot_host",
            "confirmation":"REBOOT node-abcd",
            "delay_seconds":0
        }))
        .is_err());
    }

    #[test]
    fn service_ids_map_only_to_fixed_systemd_units() {
        assert_eq!(ServiceId::Caddy.systemd_unit(), "caddy.service");
        assert_eq!(
            ServiceId::Platform.systemd_unit(),
            "vapor-platform-server.service"
        );
    }
}
