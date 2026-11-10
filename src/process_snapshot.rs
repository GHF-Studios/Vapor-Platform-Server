use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(target_os = "linux")]
use std::collections::HashMap;
#[cfg(target_os = "linux")]
use std::sync::{Mutex, OnceLock};
#[cfg(target_os = "linux")]
use std::time::Instant;

pub(crate) const MAX_RETURNED_PROCESSES: usize = 512;
const MAX_TOTAL_PROCESSES: usize = 100_000;
#[cfg(target_os = "linux")]
static CPU_BASELINE: OnceLock<Mutex<HashMap<u32, (u64, u64, Instant)>>> = OnceLock::new();

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProcessRecord {
    pub(crate) pid: u32,
    pub(crate) parent_pid: u32,
    pub(crate) name: String,
    pub(crate) state: String,
    pub(crate) uid: Option<u32>,
    #[allow(dead_code)]
    #[serde(skip)]
    cpu_ticks: u64,
    #[allow(dead_code)]
    #[serde(skip)]
    start_ticks: u64,
    pub(crate) cpu_percent: Option<f64>,
    pub(crate) resident_bytes: Option<u64>,
    pub(crate) threads: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProcessSnapshot {
    pub(crate) schema_version: u32,
    pub(crate) source: String,
    pub(crate) observed_at_unix: u64,
    pub(crate) total_processes: usize,
    pub(crate) truncated: bool,
    pub(crate) processes: Vec<ProcessRecord>,
}

pub(crate) fn read_snapshot() -> Result<ProcessSnapshot, std::io::Error> {
    let root = Path::new("/proc");
    if !root.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Linux process inventory is unavailable on this host",
        ));
    }
    let mut processes = Vec::new();
    for entry in fs::read_dir(root)? {
        let Ok(entry) = entry else { continue };
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        let process_dir = entry.path();
        let Ok(stat) = fs::read_to_string(process_dir.join("stat")) else {
            continue;
        };
        let Some(mut process) = parse_proc_stat(pid, &stat) else {
            continue;
        };
        if let Ok(status) = fs::read_to_string(process_dir.join("status")) {
            (process.uid, process.resident_bytes) = parse_status(&status);
        }
        processes.push(process);
    }
    update_cpu_percent(&mut processes);
    processes.sort_unstable_by(|left, right| {
        right
            .resident_bytes
            .cmp(&left.resident_bytes)
            .then_with(|| left.pid.cmp(&right.pid))
    });
    let (total_processes, truncated, processes) = limit_processes(processes);
    Ok(ProcessSnapshot {
        schema_version: 1,
        source: "procfs".to_owned(),
        observed_at_unix: unix_now(),
        total_processes,
        truncated,
        processes,
    })
}

fn parse_proc_stat(pid: u32, stat: &str) -> Option<ProcessRecord> {
    let (stat_pid, rest) = stat.split_once(" (")?;
    if stat_pid.parse::<u32>().ok()? != pid {
        return None;
    }
    let (name, tail) = rest.rsplit_once(") ")?;
    let fields: Vec<&str> = tail.split_whitespace().collect();
    if fields.len() <= 21 || fields[0].len() != 1 {
        return None;
    }
    let number = |index: usize| {
        fields
            .get(index)
            .and_then(|value| value.parse::<u64>().ok())
    };
    Some(ProcessRecord {
        pid,
        parent_pid: u32::try_from(number(1)?).ok()?,
        name: name.chars().take(128).collect(),
        state: fields[0].to_owned(),
        uid: None,
        cpu_ticks: number(11)?.saturating_add(number(12)?),
        start_ticks: number(19)?,
        cpu_percent: None,
        resident_bytes: None,
        threads: u32::try_from(number(17)?).ok()?,
    })
}

fn parse_status(status: &str) -> (Option<u32>, Option<u64>) {
    let uid = status
        .lines()
        .find(|line| line.starts_with("Uid:"))
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u32>().ok());
    let resident_bytes = status
        .lines()
        .find(|line| line.starts_with("VmRSS:"))
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u64>().ok())
        .and_then(|kilobytes| kilobytes.checked_mul(1024));
    (uid, resident_bytes)
}

fn limit_processes(mut processes: Vec<ProcessRecord>) -> (usize, bool, Vec<ProcessRecord>) {
    let total = processes.len();
    let truncated = total > MAX_RETURNED_PROCESSES;
    processes.truncate(MAX_RETURNED_PROCESSES);
    (total, truncated, processes)
}

pub(crate) fn valid_snapshot(snapshot: &ProcessSnapshot, now: i64) -> bool {
    if snapshot.schema_version != 1
        || snapshot.source != "procfs"
        || snapshot.observed_at_unix > now.saturating_add(60).max(0) as u64
        || snapshot.observed_at_unix < now.saturating_sub(180).max(0) as u64
        || snapshot.total_processes > MAX_TOTAL_PROCESSES
        || snapshot.processes.len() > MAX_RETURNED_PROCESSES
        || snapshot.truncated != (snapshot.total_processes > snapshot.processes.len())
    {
        return false;
    }
    let mut pids = HashSet::with_capacity(snapshot.processes.len());
    snapshot.processes.iter().all(|process| {
        process.pid > 0
            && process.parent_pid <= MAX_TOTAL_PROCESSES as u32 * 100
            && !process.name.is_empty()
            && process.name.len() <= 128
            && !process.name.chars().any(char::is_control)
            && process.state.len() == 1
            && process.state.as_bytes()[0].is_ascii_graphic()
            && process.threads <= 100_000
            && process
                .resident_bytes
                .is_none_or(|bytes| bytes <= i64::MAX as u64)
            && process
                .cpu_percent
                .is_none_or(|value| value.is_finite() && (0.0..=1_000_000.0).contains(&value))
            && pids.insert(process.pid)
    })
}

fn update_cpu_percent(processes: &mut [ProcessRecord]) {
    #[cfg(target_os = "linux")]
    {
        let ticks_per_second = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
        if ticks_per_second <= 0 {
            return;
        }
        let now = Instant::now();
        let baseline = CPU_BASELINE.get_or_init(|| Mutex::new(HashMap::new()));
        let Ok(mut baseline) = baseline.lock() else {
            return;
        };
        for process in processes.iter_mut() {
            if let Some((previous_start, previous_ticks, previous_sample)) =
                baseline.get(&process.pid)
            {
                let elapsed = now.duration_since(*previous_sample).as_secs_f64();
                if elapsed >= 0.1
                    && process.start_ticks == *previous_start
                    && process.cpu_ticks >= *previous_ticks
                {
                    let cpu_seconds =
                        (process.cpu_ticks - *previous_ticks) as f64 / ticks_per_second as f64;
                    let percent = cpu_seconds / elapsed * 100.0;
                    if percent.is_finite() {
                        process.cpu_percent = Some(percent);
                    }
                }
            }
            baseline.insert(process.pid, (process.start_ticks, process.cpu_ticks, now));
        }
        let current_pids: HashSet<u32> = processes.iter().map(|process| process.pid).collect();
        baseline.retain(|pid, _| current_pids.contains(pid));
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = processes;
    }
}

pub(crate) fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::{
        limit_processes, parse_proc_stat, parse_status, valid_snapshot, ProcessRecord,
        ProcessSnapshot, MAX_RETURNED_PROCESSES,
    };

    #[test]
    fn proc_stat_parses_names_with_spaces_and_parentheses() {
        let parsed = parse_proc_stat(
            42,
            "42 (worker with (paren) name) S 1 2 3 4 5 6 7 8 9 10 120 30 13 14 15 16 4 18 999 2048 3",
        )
        .unwrap();
        assert_eq!(parsed.pid, 42);
        assert_eq!(parsed.parent_pid, 1);
        assert_eq!(parsed.name, "worker with (paren) name");
        assert_eq!(parsed.state, "S");
        assert_eq!(parsed.cpu_ticks, 150);
        assert_eq!(parsed.start_ticks, 999);
        assert_eq!(parsed.threads, 4);
    }

    #[test]
    fn proc_status_parses_uid_and_resident_memory_in_bytes() {
        let (uid, resident_bytes) =
            parse_status("Name:\ttest\nUid:\t1001\t1001\t1001\t1001\nVmRSS:\t17 kB\n");
        assert_eq!(uid, Some(1001));
        assert_eq!(resident_bytes, Some(17 * 1024));

        let (uid, resident_bytes) =
            parse_status("Name:\ttest\nUid:\tinvalid\nVmRSS:\t999999999999999999999 kB\n");
        assert_eq!(uid, None);
        assert_eq!(resident_bytes, None);
    }

    #[test]
    fn process_limit_reports_total_and_truncates_to_the_wire_bound() {
        let processes = (0..MAX_RETURNED_PROCESSES + 3)
            .map(|pid| ProcessRecord {
                pid: pid as u32 + 1,
                parent_pid: 1,
                name: format!("process-{pid}"),
                state: "S".to_owned(),
                uid: Some(1000),
                cpu_ticks: 0,
                start_ticks: pid as u64,
                cpu_percent: None,
                resident_bytes: None,
                threads: 1,
            })
            .collect();
        let (total, truncated, processes) = limit_processes(processes);
        assert_eq!(total, MAX_RETURNED_PROCESSES + 3);
        assert!(truncated);
        assert_eq!(processes.len(), MAX_RETURNED_PROCESSES);
    }

    fn snapshot() -> ProcessSnapshot {
        ProcessSnapshot {
            schema_version: 1,
            source: "procfs".to_owned(),
            observed_at_unix: 1_000,
            total_processes: 1,
            truncated: false,
            processes: vec![ProcessRecord {
                pid: 4,
                parent_pid: 1,
                name: "vapor-node-agent".to_owned(),
                state: "S".to_owned(),
                uid: Some(0),
                cpu_ticks: 0,
                start_ticks: 0,
                cpu_percent: Some(1.5),
                resident_bytes: Some(4096),
                threads: 2,
            }],
        }
    }

    #[test]
    fn accepts_bounded_process_inventory_without_command_details() {
        assert!(valid_snapshot(&snapshot(), 1_000));
        let json = serde_json::to_value(snapshot()).unwrap();
        assert!(json["processes"][0].get("cpu_ticks").is_none());
        assert!(json["processes"][0].get("start_ticks").is_none());
        for private_field in ["cmdline", "argv", "exe", "executable", "environment", "env"] {
            assert!(json["processes"][0].get(private_field).is_none());
        }
    }

    #[test]
    fn rejects_duplicate_ids_invalid_cpu_and_unbounded_inventory() {
        let mut value = snapshot();
        value.processes.push(value.processes[0].clone());
        value.total_processes = 2;
        assert!(!valid_snapshot(&value, 1_000));

        let mut value = snapshot();
        value.processes[0].cpu_percent = Some(f64::NAN);
        assert!(!valid_snapshot(&value, 1_000));

        let mut value = snapshot();
        value.total_processes = 1_000_000;
        value.truncated = true;
        assert!(!valid_snapshot(&value, 1_000));

        let mut value = snapshot();
        value.total_processes = MAX_RETURNED_PROCESSES + 1;
        value.truncated = false;
        value.processes = (0..=MAX_RETURNED_PROCESSES)
            .map(|pid| ProcessRecord {
                pid: pid as u32 + 1,
                parent_pid: 1,
                name: format!("process-{pid}"),
                state: "S".to_owned(),
                uid: Some(1000),
                cpu_ticks: 0,
                start_ticks: pid as u64,
                cpu_percent: None,
                resident_bytes: None,
                threads: 1,
            })
            .collect();
        assert!(!valid_snapshot(&value, 1_000));
    }

    #[test]
    fn rejects_command_line_fields_in_uploaded_snapshots() {
        let mut json = serde_json::to_value(snapshot()).unwrap();
        json["processes"][0]["cmdline"] = serde_json::json!("secret arguments");
        assert!(serde_json::from_value::<ProcessSnapshot>(json).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn samples_a_valid_live_procfs_inventory() {
        let value = super::read_snapshot().expect("procfs should be available on Linux");
        assert!(value.total_processes > 0);
        assert!(valid_snapshot(&value, super::unix_now() as i64));
    }
}
