use serde::{Deserialize, Serialize};

/// Small, non-secret status returned by the root-owned Platform helper.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecoveryStatus {
    pub schema_version: u8,
    pub timer_state: String,
    pub next_run_display: Option<String>,
    pub archive_count: u32,
    pub latest_archive: Option<LatestArchive>,
    pub freshness: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LatestArchive {
    pub modified_at_unix: i64,
    pub size_bytes: u64,
    pub commit_short: String,
    pub checksum_sidecar_present: bool,
}

impl RecoveryStatus {
    pub(crate) fn validate(&self) -> bool {
        self.schema_version == 1
            && matches!(self.timer_state.as_str(), "active" | "inactive" | "unknown")
            && self
                .next_run_display
                .as_deref()
                .is_none_or(|value| {
                    value.len() <= 256 && !value.chars().any(char::is_control)
                })
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
    }
}
