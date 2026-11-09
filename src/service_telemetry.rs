use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

const MAX_CGROUP_FILE_BYTES: usize = 1024;
const MAX_SERVICE_METRICS: usize = 9;
const MAX_COUNTER: u64 = i64::MAX as u64;
const MAX_CPU_MILLICORES: u32 = 1_000_000;

const SERVICE_UNITS: &[(&str, &str)] = &[
    ("homepage", "vapor-homepage.service"),
    ("docs", "vapor-docs.service"),
    ("identity", "vapor-identity.service"),
    ("diagnostics", "vapor-diagnostics.service"),
    ("registry", "vapor-registry.service"),
    ("artifact", "vapor-artifact.service"),
    ("build-cache", "vapor-build-cache.service"),
    ("platform", "vapor-platform-server.service"),
    ("caddy", "caddy.service"),
];

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ServiceMetrics {
    pub(crate) service_id: String,
    pub(crate) available: bool,
    pub(crate) cpu_usage_usec: Option<u64>,
    pub(crate) cpu_millicores: Option<u32>,
    pub(crate) memory_current_bytes: Option<u64>,
    pub(crate) io_read_bytes: Option<u64>,
    pub(crate) io_write_bytes: Option<u64>,
}

pub(crate) fn allowed_service_id(service_id: &str) -> bool {
    SERVICE_UNITS
        .iter()
        .any(|(allowed, _)| *allowed == service_id)
}

pub(crate) fn valid_service_snapshot(snapshot: &[ServiceMetrics]) -> bool {
    snapshot.len() == MAX_SERVICE_METRICS
        && snapshot.iter().all(|metric| {
            allowed_service_id(&metric.service_id)
                && metric
                    .cpu_usage_usec
                    .is_none_or(|value| value <= MAX_COUNTER)
                && metric
                    .cpu_millicores
                    .is_none_or(|value| value <= MAX_CPU_MILLICORES)
                && metric
                    .memory_current_bytes
                    .is_none_or(|value| value <= MAX_COUNTER)
                && metric
                    .io_read_bytes
                    .is_none_or(|value| value <= MAX_COUNTER)
                && metric
                    .io_write_bytes
                    .is_none_or(|value| value <= MAX_COUNTER)
                && (metric.available
                    || (metric.cpu_usage_usec.is_none()
                        && metric.cpu_millicores.is_none()
                        && metric.memory_current_bytes.is_none()
                        && metric.io_read_bytes.is_none()
                        && metric.io_write_bytes.is_none()))
        })
        && snapshot.iter().enumerate().all(|(index, item)| {
            snapshot[..index]
                .iter()
                .all(|prior| prior.service_id != item.service_id)
        })
}

pub(crate) fn read_service_snapshot(
    cgroup_root: &Path,
    previous: Option<&[ServiceMetrics]>,
    elapsed: Duration,
) -> Vec<ServiceMetrics> {
    SERVICE_UNITS
        .iter()
        .map(|(service_id, unit)| {
            let directory = cgroup_root.join(unit);
            if !std::fs::symlink_metadata(&directory)
                .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
            {
                return unavailable(service_id);
            }
            let cpu_usage_usec = read_cgroup_file(&directory.join("cpu.stat"))
                .and_then(|contents| parse_usage_usec(&contents));
            let prior_cpu = previous
                .and_then(|items| items.iter().find(|item| item.service_id == *service_id))
                .and_then(|item| item.cpu_usage_usec);
            let cpu_millicores = cpu_usage_usec
                .zip(prior_cpu)
                .and_then(|(current, prior)| cpu_millicores(prior, current, elapsed));
            let memory_current_bytes = read_cgroup_file(&directory.join("memory.current"))
                .and_then(|contents| parse_memory_current(&contents));
            let (io_read_bytes, io_write_bytes) = read_cgroup_file(&directory.join("io.stat"))
                .and_then(|contents| parse_io_stat(&contents))
                .map(|(read, write)| (Some(read), Some(write)))
                .unwrap_or((None, None));
            ServiceMetrics {
                service_id: (*service_id).to_owned(),
                available: true,
                cpu_usage_usec,
                cpu_millicores,
                memory_current_bytes,
                io_read_bytes,
                io_write_bytes,
            }
        })
        .collect()
}

fn unavailable(service_id: &str) -> ServiceMetrics {
    ServiceMetrics {
        service_id: service_id.to_owned(),
        available: false,
        cpu_usage_usec: None,
        cpu_millicores: None,
        memory_current_bytes: None,
        io_read_bytes: None,
        io_write_bytes: None,
    }
}

fn read_cgroup_file(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let mut contents = String::new();
    file.by_ref()
        .take((MAX_CGROUP_FILE_BYTES + 1) as u64)
        .read_to_string(&mut contents)
        .ok()?;
    (contents.len() <= MAX_CGROUP_FILE_BYTES).then_some(contents)
}

fn parse_usage_usec(contents: &str) -> Option<u64> {
    if contents.len() > MAX_CGROUP_FILE_BYTES {
        return None;
    }
    let mut value = None;
    for line in contents.lines() {
        let mut parts = line.split_whitespace();
        if parts.next()? == "usage_usec" {
            let parsed = parts.next()?.parse::<u64>().ok()?;
            if parts.next().is_some() || parsed > MAX_COUNTER || value.replace(parsed).is_some() {
                return None;
            }
        }
    }
    value
}

fn parse_memory_current(contents: &str) -> Option<u64> {
    if contents.len() > MAX_CGROUP_FILE_BYTES {
        return None;
    }
    let value = contents.trim().parse::<u64>().ok()?;
    (value <= MAX_COUNTER).then_some(value)
}

fn parse_io_stat(contents: &str) -> Option<(u64, u64)> {
    if contents.len() > MAX_CGROUP_FILE_BYTES {
        return None;
    }
    let mut totals = (0_u64, 0_u64);
    let mut rows = 0_usize;
    for line in contents.lines() {
        let mut fields = line.split_whitespace();
        let device = fields.next()?;
        let (major, minor) = device.split_once(':')?;
        if major.parse::<u32>().is_err() || minor.parse::<u32>().is_err() {
            return None;
        }
        let mut read = None;
        let mut write = None;
        for field in fields {
            if let Some(value) = field.strip_prefix("rbytes=") {
                read = Some(value.parse::<u64>().ok()?);
            } else if let Some(value) = field.strip_prefix("wbytes=") {
                write = Some(value.parse::<u64>().ok()?);
            }
        }
        totals.0 = totals.0.checked_add(read?)?;
        totals.1 = totals.1.checked_add(write?)?;
        rows += 1;
    }
    (rows > 0 && totals.0 <= MAX_COUNTER && totals.1 <= MAX_COUNTER).then_some(totals)
}

fn cpu_millicores(previous_usec: u64, current_usec: u64, elapsed: Duration) -> Option<u32> {
    let elapsed_usec = elapsed.as_micros();
    if elapsed_usec == 0 {
        return None;
    }
    let delta_usec = current_usec.checked_sub(previous_usec)? as u128;
    let rate = delta_usec.checked_mul(1000)?.checked_div(elapsed_usec)?;
    (rate <= MAX_CPU_MILLICORES as u128).then_some(rate as u32)
}

#[cfg(test)]
mod tests {
    use super::{
        ServiceMetrics, cpu_millicores, parse_io_stat, parse_memory_current, parse_usage_usec,
        read_service_snapshot, valid_service_snapshot,
    };
    use std::fs;
    use std::time::Duration;

    #[test]
    fn parses_bounded_cgroup_v2_resource_counters() {
        assert_eq!(
            parse_usage_usec("usage_usec 2000000\nuser_usec 100\n"),
            Some(2_000_000)
        );
        assert_eq!(parse_memory_current("4096\n"), Some(4096));
        assert_eq!(
            parse_io_stat("8:0 rbytes=10 wbytes=20 rios=1\n8:16 rbytes=2 wbytes=3\n"),
            Some((12, 23))
        );
    }

    #[test]
    fn malformed_or_unbounded_counter_files_are_unavailable() {
        let oversized = "x".repeat(1025);
        assert_eq!(parse_usage_usec("usage_usec -1\n"), None);
        assert_eq!(parse_memory_current("18446744073709551616\n"), None);
        assert_eq!(parse_io_stat("8:0 rbytes=1 wbytes=not-a-number\n"), None);
        assert_eq!(parse_memory_current(&oversized), None);
    }

    #[test]
    fn cpu_rate_uses_millicores_and_rejects_counter_reset() {
        assert_eq!(
            cpu_millicores(1_000_000, 3_000_000, Duration::from_secs(2)),
            Some(1000)
        );
        assert_eq!(
            cpu_millicores(2_000_000, 1_000_000, Duration::from_secs(2)),
            None
        );
        assert_eq!(cpu_millicores(0, 100, Duration::ZERO), None);
    }

    #[test]
    fn absent_units_are_explicitly_unavailable_and_snapshot_is_allowlisted() {
        let root = fixture_root();
        let snapshot = read_service_snapshot(&root, None, Duration::ZERO);
        assert_eq!(snapshot.len(), 9);
        assert!(snapshot.iter().all(|metric| !metric.available));
        assert!(valid_service_snapshot(&snapshot));
        let mut injected = snapshot;
        injected[0].service_id = "../../etc/passwd".to_owned();
        assert!(!valid_service_snapshot(&injected));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reads_only_the_fixed_unit_and_calculates_interval_usage() {
        let root = fixture_root();
        let docs = root.join("vapor-docs.service");
        fs::create_dir_all(&docs).unwrap();
        fs::write(
            docs.join("cpu.stat"),
            "usage_usec 1000000\nuser_usec 500000\n",
        )
        .unwrap();
        fs::write(docs.join("memory.current"), "4096\n").unwrap();
        fs::write(docs.join("io.stat"), "8:0 rbytes=50 wbytes=75 rios=1\n").unwrap();
        let first = read_service_snapshot(&root, None, Duration::from_secs(1));
        let docs_first = first
            .iter()
            .find(|metric| metric.service_id == "docs")
            .unwrap();
        assert!(docs_first.available);
        assert_eq!(docs_first.cpu_millicores, None);
        assert_eq!(docs_first.memory_current_bytes, Some(4096));
        fs::write(docs.join("cpu.stat"), "usage_usec 3000000\n").unwrap();
        let second = read_service_snapshot(&root, Some(&first), Duration::from_secs(2));
        let docs_second = second
            .iter()
            .find(|metric| metric.service_id == "docs")
            .unwrap();
        assert_eq!(docs_second.cpu_millicores, Some(1000));
        assert_eq!(docs_second.io_read_bytes, Some(50));
        assert_eq!(docs_second.io_write_bytes, Some(75));
        assert!(valid_service_snapshot(&second));
        let _ = fs::remove_dir_all(root);
    }

    fn fixture_root() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "vapor-service-telemetry-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn duplicate_service_ids_are_rejected() {
        let item = ServiceMetrics {
            service_id: "docs".to_owned(),
            available: false,
            cpu_usage_usec: None,
            cpu_millicores: None,
            memory_current_bytes: None,
            io_read_bytes: None,
            io_write_bytes: None,
        };
        assert!(!valid_service_snapshot(&vec![item.clone(); 9]));
    }
}
