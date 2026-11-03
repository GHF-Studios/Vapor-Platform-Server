use std::fmt;

const MAX_DURATION_SECONDS: i64 = 8 * 60 * 60;
const CAPABILITIES: &[&str] = &["vapor_deploy", "vapor_service_control", "host_admin"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SshCommand {
    Help,
    List,
    Show {
        request_id: String,
    },
    Grant {
        request_id: String,
        capabilities: Vec<String>,
        duration_seconds: i64,
    },
    Revoke {
        request_id: String,
    },
    Reconfigure {
        request_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliParseError(String);

impl fmt::Display for CliParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for CliParseError {}

pub fn usage() -> &'static str {
    "Usage:\n  vapor-platform-ops ssh list\n  vapor-platform-ops ssh show <request-id>\n  vapor-platform-ops ssh grant <request-id> --capability <name> [--capability <name> ...] --duration <1..8h>\n  vapor-platform-ops ssh revoke <request-id>\n  vapor-platform-ops ssh reconfigure <request-id>\n\nCapabilities: vapor_deploy, vapor_service_control, host_admin (exclusive)\nDurations accept seconds, minutes, or hours (for example: 1800, 30m, 8h).\nMutations require root and an interactive confirmation. Grants require typing the displayed fingerprint. Reconfigure requires revoke and a new request."
}

pub fn parse_ssh_args(args: &[String]) -> Result<SshCommand, CliParseError> {
    let Some(command) = args.first().map(String::as_str) else {
        return Err(CliParseError("missing SSH operation".to_owned()));
    };
    if args.len() == 1 && matches!(command, "--help" | "-h" | "help") {
        return Ok(SshCommand::Help);
    }
    match command {
        "list" if args.len() == 1 => Ok(SshCommand::List),
        "show" if args.len() == 2 => Ok(SshCommand::Show {
            request_id: parse_request_id(&args[1])?,
        }),
        "grant" => parse_grant_args(&args[1..]),
        "revoke" if args.len() == 2 => Ok(SshCommand::Revoke {
            request_id: parse_request_id(&args[1])?,
        }),
        "reconfigure" if args.len() == 2 => Ok(SshCommand::Reconfigure {
            request_id: parse_request_id(&args[1])?,
        }),
        "list" | "show" | "revoke" | "reconfigure" => {
            Err(CliParseError("unexpected or missing argument".to_owned()))
        }
        _ => Err(CliParseError("unknown SSH operation".to_owned())),
    }
}

fn parse_grant_args(args: &[String]) -> Result<SshCommand, CliParseError> {
    let Some(request_id) = args.first() else {
        return Err(CliParseError("grant requires a request ID".to_owned()));
    };
    let request_id = parse_request_id(request_id)?;
    let mut capabilities = Vec::new();
    let mut duration_seconds = None;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--capability" => {
                let capability = args
                    .get(index + 1)
                    .ok_or_else(|| CliParseError("--capability requires a value".to_owned()))?;
                if !CAPABILITIES.contains(&capability.as_str()) {
                    return Err(CliParseError("unsupported capability".to_owned()));
                }
                if capabilities.contains(capability) {
                    return Err(CliParseError("duplicate capability".to_owned()));
                }
                capabilities.push(capability.clone());
                index += 2;
            }
            "--duration" => {
                if duration_seconds.is_some() {
                    return Err(CliParseError("duplicate --duration".to_owned()));
                }
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| CliParseError("--duration requires a value".to_owned()))?;
                duration_seconds = Some(parse_duration(value)?);
                index += 2;
            }
            _ => return Err(CliParseError("unknown grant option".to_owned())),
        }
    }
    if capabilities.is_empty() {
        return Err(CliParseError(
            "grant requires at least one capability".to_owned(),
        ));
    }
    if capabilities
        .iter()
        .any(|capability| capability == "host_admin")
        && capabilities.len() != 1
    {
        return Err(CliParseError(
            "host_admin must be granted by itself".to_owned(),
        ));
    }
    let duration_seconds =
        duration_seconds.ok_or_else(|| CliParseError("grant requires --duration".to_owned()))?;
    Ok(SshCommand::Grant {
        request_id,
        capabilities,
        duration_seconds,
    })
}

fn parse_request_id(value: &str) -> Result<String, CliParseError> {
    uuid::Uuid::parse_str(value)
        .map(|id| id.to_string())
        .map_err(|_| CliParseError("invalid request ID".to_owned()))
}

fn parse_duration(value: &str) -> Result<i64, CliParseError> {
    let suffix_at = value
        .bytes()
        .position(|byte| !byte.is_ascii_digit())
        .unwrap_or(value.len());
    let (number, suffix) = value.split_at(suffix_at);
    let number = number
        .parse::<i64>()
        .map_err(|_| CliParseError("invalid duration".to_owned()))?;
    let multiplier = match suffix {
        "" | "s" => 1,
        "m" => 60,
        "h" => 60 * 60,
        _ => return Err(CliParseError("invalid duration suffix".to_owned())),
    };
    let seconds = number
        .checked_mul(multiplier)
        .filter(|seconds| (1..=MAX_DURATION_SECONDS).contains(seconds))
        .ok_or_else(|| CliParseError("duration must be from 1 second to 8 hours".to_owned()))?;
    Ok(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REQUEST_ID: &str = "fb9ef006-f6ad-420c-9ad7-31337b2b1e52";

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn parses_requested_bootstrap_scope_and_hours() {
        let parsed = parse_ssh_args(&args(&[
            "grant",
            REQUEST_ID,
            "--capability",
            "vapor_deploy",
            "--capability",
            "vapor_service_control",
            "--duration",
            "8h",
        ]))
        .unwrap();

        assert_eq!(
            parsed,
            SshCommand::Grant {
                request_id: REQUEST_ID.to_owned(),
                capabilities: vec![
                    "vapor_deploy".to_owned(),
                    "vapor_service_control".to_owned()
                ],
                duration_seconds: 28_800,
            }
        );
    }

    #[test]
    fn rejects_invalid_id_capability_conflict_and_oversized_duration() {
        assert!(parse_ssh_args(&args(&["show", "bad-id"])).is_err());
        assert!(parse_ssh_args(&args(&[
            "grant",
            REQUEST_ID,
            "--capability",
            "not-a-capability",
            "--duration",
            "1h",
        ]))
        .unwrap_err()
        .to_string()
        .contains("unsupported capability"));
        assert!(parse_ssh_args(&args(&[
            "grant",
            REQUEST_ID,
            "--capability",
            "host_admin",
            "--capability",
            "vapor_deploy",
            "--duration",
            "1h",
        ]))
        .unwrap_err()
        .to_string()
        .contains("by itself"));
        assert!(parse_ssh_args(&args(&[
            "grant",
            REQUEST_ID,
            "--capability",
            "vapor_deploy",
            "--duration",
            "9h",
        ]))
        .unwrap_err()
        .to_string()
        .contains("8 hours"));
    }

    #[test]
    fn parses_read_only_and_mutating_commands_and_reconfigure() {
        assert_eq!(parse_ssh_args(&args(&["list"])).unwrap(), SshCommand::List);
        assert_eq!(
            parse_ssh_args(&args(&["show", REQUEST_ID])).unwrap(),
            SshCommand::Show {
                request_id: REQUEST_ID.to_owned()
            }
        );
        assert_eq!(
            parse_ssh_args(&args(&["revoke", REQUEST_ID])).unwrap(),
            SshCommand::Revoke {
                request_id: REQUEST_ID.to_owned()
            }
        );
        assert_eq!(
            parse_ssh_args(&args(&["reconfigure", REQUEST_ID])).unwrap(),
            SshCommand::Reconfigure {
                request_id: REQUEST_ID.to_owned()
            }
        );
        assert_eq!(
            parse_ssh_args(&args(&[
                "grant",
                REQUEST_ID,
                "--capability",
                "vapor_deploy",
                "--duration",
                "30m"
            ]))
            .unwrap(),
            SshCommand::Grant {
                request_id: REQUEST_ID.to_owned(),
                capabilities: vec!["vapor_deploy".to_owned()],
                duration_seconds: 1800
            }
        );
    }

    #[test]
    fn rejects_duplicate_capabilities_duration_and_unknown_options() {
        let duplicate_capability = args(&[
            "grant",
            REQUEST_ID,
            "--capability",
            "vapor_deploy",
            "--capability",
            "vapor_deploy",
            "--duration",
            "1h",
        ]);
        assert!(parse_ssh_args(&duplicate_capability)
            .unwrap_err()
            .to_string()
            .contains("duplicate capability"));

        let duplicate_duration = args(&[
            "grant",
            REQUEST_ID,
            "--capability",
            "vapor_deploy",
            "--duration",
            "1h",
            "--duration",
            "2h",
        ]);
        assert!(parse_ssh_args(&duplicate_duration)
            .unwrap_err()
            .to_string()
            .contains("duplicate --duration"));

        let unknown_option = args(&[
            "grant",
            REQUEST_ID,
            "--capability",
            "vapor_deploy",
            "--duration",
            "1h",
            "--force",
        ]);
        assert!(parse_ssh_args(&unknown_option).is_err());
        assert!(parse_ssh_args(&args(&["list", "--all"])).is_err());
    }
}
