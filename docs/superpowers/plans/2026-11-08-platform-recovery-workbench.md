# Platform Recovery Workbench Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Give root operators a task-focused view of VPS backup freshness and explicit recovery evidence gaps.

**Architecture:** A fixed, read-only `recovery-status` subcommand in the root-owned Platform helper reports a small JSON summary. A root-protected Platform route invokes that exact command and validates the bounded response; the shared dashboard renders the summary and its evidence limitations. No arbitrary command, archive read, restore, or destructive operation is exposed.

**Tech Stack:** Rust, Tokio/Axum, serde, systemd, existing Rust and Python test recipes, embedded admin JavaScript.

**Spec:** `docs/superpowers/specs/2026-11-08-platform-recovery-workbench-design.md`

## Global Constraints

- Platform owns host setup, whole-system export/import, and recovery orchestration.
- Service business logic stays in its root-level submodule.
- The admin page is the human interface; APIs and the fixed helper enforce authorization and own canonical state.
- No arbitrary browser shell or raw host path, archive content, environment value, or credential is returned.
- USB copy and clean-host restore status remain unverified unless explicit evidence is recorded.
- The read-only status route requires a root Identity session and returns `private, no-store`.

## Review Focus

- Missing, stale, symlinked, or malformed archive entries: helper must fail closed and never count unsafe files as backups.
- Inactive or unreadable systemd timer: UI must say attention/unknown, not ready.
- Missing helper, timeout, oversized output, or unknown JSON fields: API returns a bounded unavailable response without echoing helper output.
- Unauthorized caller: helper must not run before root authorization completes.
- USB and restore claims: UI must continue to say not reported/not recorded absent explicit evidence.

---

### Task 1: Add a bounded recovery-status data model and fixed helper action

**Files:**
- Create: `src/recovery_status.rs`
- Modify: `src/bin/vapor_platform_ops.rs`
- Modify: `deploy/scripts/install-platform-ops.sh`
- Test: `src/recovery_status.rs` unit tests and `deploy/tests/test_platform_ops_installer.py`

**Interfaces:**
- Produces: exact helper invocation `/usr/local/libexec/vapor-platform-ops recovery-status`; no additional arguments are accepted.
- JSON fields: `schema_version`, `timer_state`, bounded `next_run_display`, `archive_count`, `latest_archive` (`modified_at_unix`, `size_bytes`, `commit_short`, `checksum_sidecar_present`), and `freshness` (`recent`, `stale`, or `unknown`). A timestamp over five minutes in the future is unknown. Recent means recent by mtime only, not validated.

- [ ] Cover missing archive root, valid archive + sidecar, symlink exclusion, future/stale timestamps, malformed/missing metadata, and fixed-path command output.
- [ ] Implement fixed-path, bounded archive enumeration and systemd timer observation; emit only the documented JSON fields.
- [ ] Add one exact sudoers command allowance for the fixed subcommand, retaining the existing no-argument stdin-only allowance for interactive operations.
- [ ] Run focused Rust and installer tests.
- [ ] Commit the helper slice.

### Task 2: Add the root-only recovery status API

**Files:**
- Create: `src/recovery.rs`
- Modify: `src/main.rs`
- Test: `src/recovery.rs` unit tests

**Interfaces:**
- Produces: `GET /api/admin/recovery/status`.
- Consumes: validated root session and fixed helper JSON.
- Rejects non-root before starting the helper; timeout and response size are bounded.

- [ ] Cover missing/unknown JSON fields, oversized output, helper failure, private no-store cache headers, and authorization-before-helper ordering.
- [ ] Implement the fixed subprocess call and strict response validation; do not include stderr/stdout details in errors.
- [ ] Merge the recovery router into Platform in both monolith and services mode.
- [ ] Commit the API slice.

### Task 3: Add the Recovery dashboard workbench

**Files:**
- Modify: `src/admin.rs`
- Test: `src/admin.rs` route/navigation/embedded-script tests

**Interfaces:**
- Adds `/admin/recovery` under a distinct Emergency navigation group.
- Displays VPS archive freshness/count/size/revision, timer state, checksum-sidecar presence, USB reporting state, and restore-drill evidence state.
- Never presents archive presence as a successful restore.

- [ ] Cover navigation, page registration, clear unverified labels, API fetch contract, and stale/missing states.
- [ ] Implement concise cards with explicit statuses and a link to the runbook.
- [ ] Parse-check the embedded JavaScript and run the full Platform test recipe.
- [ ] Commit the dashboard slice.

### Task 4: Review and verify

- [ ] Verify `deploy/scripts/test.sh` and the embedded JavaScript syntax check.
- [ ] Run the branch CI checks on Windows and Linux.
- [ ] Request a read-only adversarial review of privilege ordering, data exposure, stale/missing semantics, and UI claims.
- [ ] Push to the feature branch only; do not replace or bypass the pending owner-approved deployment request for the previous main commit.

## Self-review

- **Spec coverage:** fixed helper, strict API, dashboard, explicit unknown/unverified states, no destructive controls, and owner-authorized deploy boundary each have a task.
- **Review focus:** all five stated failure classes map to helper/API/UI tests.
- **Scope:** USB report ingestion and restore execution are intentionally excluded because they require separate authenticated-source and destructive-operation designs; the page states both are unverified.
- **Compatibility:** the pre-alpha helper invocation is new and exact; no broad compatibility command is introduced.

## Follow-on work

The owner also requested a browser-based emergency console. Its separate
security contract is in
`docs/superpowers/specs/2026-11-08-emergency-console-design.md`. Do not expose
an interactive shell from this recovery implementation; implement the
console as its own reviewed service/authorization slice.
