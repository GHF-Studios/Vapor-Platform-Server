use serde::{Deserialize, Serialize};

/// Small, non-secret status returned by the root-owned Platform helper.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecoveryStatus {
    pub schema_version: u8,
    pub archive_root_state: String,
    pub timer_state: String,
    pub next_run_display: Option<String>,
    pub archive_count: u32,
    pub latest_archive: Option<LatestArchive>,
    pub freshness: String,
    pub state_only_timer_state: String,
    pub state_only_archive_count: u32,
    pub state_only_latest_archive: Option<LatestStateOnlyArchive>,
    #[serde(default)]
    pub owner_usb_sync: Option<OwnerUsbSyncStatus>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LatestStateOnlyArchive {
    pub modified_at_unix: i64,
    pub size_bytes: u64,
    pub checksum_sidecar_present: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LatestArchive {
    pub modified_at_unix: i64,
    pub size_bytes: u64,
    pub commit_short: String,
    pub checksum_sidecar_present: bool,
}

/// A bounded report from the owner's verified local USB sync task.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OwnerUsbSyncStatus {
    pub last_success_unix: i64,
    pub latest_archive: String,
    pub latest_archive_sha256: String,
    pub verified_archive_count: u32,
    pub unverified_archive_count: u32,
    pub retained_archive_payload_bytes: u64,
    pub latest_rehashed: bool,
    pub recovery_kit_available: bool,
    pub recovery_kit_preflight_validation: bool,
    pub recovery_kit_source_commit: Option<String>,
    pub freshness: String,
}

impl OwnerUsbSyncStatus {
    fn validate_at(&self, now_unix: i64) -> bool {
        let expected_freshness = if now_unix.saturating_sub(self.last_success_unix) <= 26 * 60 * 60
        {
            "recent"
        } else {
            "stale"
        };
        self.last_success_unix > 0
            && self.last_success_unix <= now_unix.saturating_add(300)
            && self.latest_archive.len() <= 128
            && valid_owner_archive_name(&self.latest_archive)
            && self.latest_archive_sha256.len() == 64
            && self
                .latest_archive_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            && self.verified_archive_count > 0
            && self.verified_archive_count <= 10_000
            && self.unverified_archive_count <= 10_000
            && self.retained_archive_payload_bytes > 0
            && self.retained_archive_payload_bytes <= (1_u64 << 40)
            && self.latest_rehashed
            && self.freshness == expected_freshness
            && if self.recovery_kit_available {
                self.recovery_kit_preflight_validation
                    && self
                        .recovery_kit_source_commit
                        .as_deref()
                        .is_some_and(|commit| {
                            commit.len() == 40
                                && commit.bytes().all(|byte| {
                                    byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()
                                })
                        })
            } else {
                !self.recovery_kit_preflight_validation && self.recovery_kit_source_commit.is_none()
            }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnerUsbSyncRecord {
    schema_version: u8,
    last_success_unix: i64,
    latest_archive: String,
    latest_archive_sha256: String,
    verified_archive_count: u32,
    unverified_archive_count: u32,
    retained_archive_payload_bytes: u64,
    latest_rehashed: bool,
    recovery_kit_available: bool,
    recovery_kit_preflight_validation: bool,
    recovery_kit_source_commit: Option<String>,
}

pub(crate) fn load_owner_usb_sync(
    root: &std::path::Path,
    now_unix: i64,
) -> Option<OwnerUsbSyncStatus> {
    let root_metadata = std::fs::symlink_metadata(root).ok()?;
    if !root_metadata.is_dir() || root_metadata.file_type().is_symlink() {
        return None;
    }
    let path = root.join("owner-usb-sync.json");
    let metadata = std::fs::symlink_metadata(&path).ok()?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() || metadata.len() > 4096
    {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() > 4096 {
        return None;
    }
    let record: OwnerUsbSyncRecord = serde_json::from_slice(&bytes).ok()?;
    if record.schema_version != 1
        || record.last_success_unix <= 0
        || record.last_success_unix > now_unix.saturating_add(300)
        || record.latest_archive.len() > 128
        || !valid_owner_archive_name(&record.latest_archive)
        || record.latest_archive_sha256.len() != 64
        || !record
            .latest_archive_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        || record.verified_archive_count == 0
        || record.verified_archive_count > 10_000
        || record.unverified_archive_count > 10_000
        || record.retained_archive_payload_bytes == 0
        || record.retained_archive_payload_bytes > (1_u64 << 40)
        || !record.latest_rehashed
    {
        return None;
    }
    if record.recovery_kit_available {
        let Some(commit) = record.recovery_kit_source_commit.as_deref() else {
            return None;
        };
        if !record.recovery_kit_preflight_validation
            || commit.len() != 40
            || !commit
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return None;
        }
    } else if record.recovery_kit_preflight_validation
        || record.recovery_kit_source_commit.is_some()
    {
        return None;
    }
    let freshness = if now_unix.saturating_sub(record.last_success_unix) <= 26 * 60 * 60 {
        "recent"
    } else {
        "stale"
    };
    let status = OwnerUsbSyncStatus {
        last_success_unix: record.last_success_unix,
        latest_archive: record.latest_archive,
        latest_archive_sha256: record.latest_archive_sha256,
        verified_archive_count: record.verified_archive_count,
        unverified_archive_count: record.unverified_archive_count,
        retained_archive_payload_bytes: record.retained_archive_payload_bytes,
        latest_rehashed: record.latest_rehashed,
        recovery_kit_available: record.recovery_kit_available,
        recovery_kit_preflight_validation: record.recovery_kit_preflight_validation,
        recovery_kit_source_commit: record.recovery_kit_source_commit,
        freshness: freshness.to_owned(),
    };
    status.validate_at(now_unix).then_some(status)
}

fn valid_owner_archive_name(name: &str) -> bool {
    let Some(value) = name
        .strip_prefix("vapor-platform-recovery-")
        .and_then(|value| value.strip_suffix(".tar.gz"))
    else {
        return false;
    };
    let Some((timestamp, commit)) = value.rsplit_once('-') else {
        return false;
    };
    let bytes = timestamp.as_bytes();
    bytes.len() == 16
        && bytes[8] == b'T'
        && bytes[15] == b'Z'
        && bytes[..8].iter().all(u8::is_ascii_digit)
        && bytes[9..15].iter().all(u8::is_ascii_digit)
        && commit.len() == 12
        && commit
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

impl RecoveryStatus {
    pub(crate) fn validate(&self) -> bool {
        self.schema_version == 1
            && matches!(
                self.archive_root_state.as_str(),
                "available" | "missing" | "unavailable"
            )
            && matches!(self.timer_state.as_str(), "active" | "inactive" | "unknown")
            && matches!(
                self.state_only_timer_state.as_str(),
                "active" | "inactive" | "unknown"
            )
            && self.state_only_archive_count <= 10_000
            && self
                .state_only_latest_archive
                .as_ref()
                .is_none_or(|archive| archive.modified_at_unix > 0 && archive.size_bytes > 0)
            && (self.state_only_latest_archive.is_some() || self.state_only_archive_count == 0)
            && self
                .next_run_display
                .as_deref()
                .is_none_or(|value| value.len() <= 256 && !value.chars().any(char::is_control))
            && self.archive_count <= 10_000
            && matches!(self.freshness.as_str(), "recent" | "stale" | "unknown")
            && self.latest_archive.as_ref().is_none_or(|archive| {
                archive.modified_at_unix > 0
                    && archive.size_bytes > 0
                    && archive.commit_short.len() == 12
                    && archive
                        .commit_short
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit())
            })
            && (self.latest_archive.is_some() || self.freshness == "unknown")
            && (self.latest_archive.is_none()
                || (self.archive_root_state == "available" && self.archive_count > 0))
            && (self.archive_root_state == "available"
                || (self.latest_archive.is_none()
                    && self.archive_count == 0
                    && self.freshness == "unknown"))
            && self.owner_usb_sync.as_ref().is_none_or(|report| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .and_then(|duration| i64::try_from(duration.as_secs()).ok())
                    .is_some_and(|now| report.validate_at(now))
            })
    }
}

#[cfg(test)]
mod owner_usb_tests {
    use super::load_owner_usb_sync;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_root() -> std::path::PathBuf {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("vapor-usb-status-{id}"));
        std::fs::create_dir(&path).unwrap();
        path
    }

    fn report(stamp: i64) -> String {
        format!(
            r#"{{"schema_version":1,"last_success_unix":{stamp},"latest_archive":"vapor-platform-recovery-20261107T120000Z-0123456789ab.tar.gz","latest_archive_sha256":"{}","verified_archive_count":12,"unverified_archive_count":0,"retained_archive_payload_bytes":1024,"latest_rehashed":true,"recovery_kit_available":true,"recovery_kit_preflight_validation":true,"recovery_kit_source_commit":"0123456789abcdef0123456789abcdef01234567"}}"#,
            "a".repeat(64)
        )
    }

    #[test]
    fn loads_recent_owner_report_and_marks_old_report_stale() {
        let root = temporary_root();
        let path = root.join("owner-usb-sync.json");
        std::fs::write(&path, report(1_000)).unwrap();
        let mut data: serde_json::Value = serde_json::from_str(&report(1_000)).unwrap();
        data["last_success_unix"] = serde_json::json!(1_000 + 25 * 3600);
        std::fs::write(&path, serde_json::to_vec(&data).unwrap()).unwrap();
        assert_eq!(
            load_owner_usb_sync(&root, 1_000 + 25 * 3600)
                .unwrap()
                .freshness,
            "recent"
        );
        data["last_success_unix"] = serde_json::json!(1_000);
        std::fs::write(&path, serde_json::to_vec(&data).unwrap()).unwrap();
        assert_eq!(
            load_owner_usb_sync(&root, 1_000 + 26 * 3600 + 1)
                .unwrap()
                .freshness,
            "stale"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_or_symlinked_reports_are_unknown() {
        let root = temporary_root();
        let path = root.join("owner-usb-sync.json");
        std::fs::write(&path, "{bad").unwrap();
        assert!(load_owner_usb_sync(&root, 2_000).is_none());
        std::fs::remove_file(&path).unwrap();
        let target = root.join("target.json");
        std::fs::write(&target, report(1_000)).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&target, &path).unwrap();
            assert!(load_owner_usb_sync(&root, 2_000).is_none());
        }
        #[cfg(windows)]
        if std::os::windows::fs::symlink_file(&target, &path).is_ok() {
            assert!(load_owner_usb_sync(&root, 2_000).is_none());
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
