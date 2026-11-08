# State-only owner recovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace recurring full-host plaintext recovery archives with frequent, visible, owner-controlled state-only recovery.

**Architecture:** Service-owned durable state is exported into a versioned archive with an allowlisted state catalog. Vapor setup rebuilds host configuration and re-enters secrets; the high-security Recovery page coordinates export, download, and transactional import. Existing full-host archives remain untouched until a new archive has been verified on USB.

**Tech Stack:** Rust/Axum admin routes, root-only Platform operations helper, Python archive validation, Bash server setup/export/restore, PowerShell USB transfer.

**Spec:** `docs/superpowers/specs/2026-11-08-state-only-owner-recovery.md`

## Global Constraints

- Keep secrets out of new archives, manifests, logs, source control, and browser-visible status.
- Keep the removable USB copy unencrypted and owner-controlled.
- Do not delete existing archive generations during format migration.
- Host and service setup is rebuilt by Vapor; state import does not install or replace host configuration.
- Keep service-owned data semantics in the owning service repositories; Platform composes their fixed state-export/import interface.

## Review Focus

- Newly added persistent service paths are excluded until explicitly classified.
- State-only restore cannot replace `/etc`, Caddy, systemd, source checkouts, or provider credentials.
- SQLite WAL/SHM files are captured consistently and restored with correct ownership.
- Download and restore routes require a root Identity session and private/no-store responses.
- Migration does not prune legacy USB archives before a verified new-format copy exists.

---

### Task 1: Inventory durable state owners

**Files:**
- Modify: `docs/superpowers/specs/2026-11-08-state-only-owner-recovery.md`
- Modify: service state contracts in root-level service submodules, only where an owner path is missing

- [x] Record each persistent path under `/var/lib/vapor-server` as authoritative state, rebuildable cache, generated log, or host configuration.
- [x] Record the authoritative export/import mechanism for each service database and published content store.
- [x] Review the inventory against the live archive member list without reading secret file contents.

### Task 2: Add a state-only archive schema and validator

**Files:**
- Modify: `deploy/scripts/export-state.sh`
- Modify: `deploy/scripts/state-bundle-validate.py`
- Modify: `deploy/scripts/recovery_bundle_metadata.py`
- Test: `deploy/tests/test_state_bundle_validation.py`

- [x] Add tests proving the new schema accepts an allowlisted state archive and rejects host/config and unclassified entries.
- [x] Add a separate state-only exporter with an explicit state allowlist, schema-specific filename, isolated pointer and index.
- [x] Validate payload checksums and bounded archive structure before publication.
- [x] Snapshot live SQLite databases through SQLite's online backup API without stopping service writers.
- [x] Keep the old archive reader isolated for inspection of retained generations; new exports never use its full-host format.

### Task 3: Make state import configuration-neutral

**Files:**
- Add: `deploy/scripts/restore-state-only.sh`
- Modify: `deploy/scripts/restore-state.sh` (legacy timer migration only)
- Modify: `deploy/scripts/recover-fresh-host.sh`
- Modify: `deploy/scripts/restore_host_preflight.py`
- Test: recovery/restore tests under `deploy/tests/`

- [x] Restore only allowlisted state paths after setup has installed the target services and the operator has configured secrets.
- [x] Preserve current state under a unique pre-restore path and roll back on failed readiness.
- [x] Reject attempts to restore a state-only archive before setup prerequisites are satisfied.
- [ ] Keep current legacy full-host archives available as a separately named, explicitly selected path only during migration.

### Task 4: Replace implicit USB sync with visible owner actions

**Files:**
- Modify: `src/recovery.rs`, `src/recovery_status.rs`, and root-only recovery operations in `src/bin/vapor_platform_ops.rs`
- Modify: `src/admin.rs`
- Modify: `deploy/scripts/backup-to-usb.ps1` and its tests

- [ ] Add bounded status fields for cadence, retention, last export, archive schema, and USB copy verification.
- [ ] Add an authenticated create/download action using a short-lived server-side file and a bounded cleanup policy.
- [ ] Add a separately confirmed restore action; show target, archive date, resulting scope, and rollback behavior before execution.
- [x] Add a separate USB sync path for verified state-only generations; it writes to `State-Snapshots/` and does not prune legacy copies.
- [x] Make the state-only USB flow explicit; it verifies indexed generations, writes to a separate directory, and never prunes old generations.
- [ ] Compact the Recovery layout around status and primary actions; put details in expandable evidence panels.

### Task 5: Move host reconstruction into Vapor setup

**Files:**
- Modify: `deploy/scripts/bootstrap-ubuntu.sh`, `deploy/scripts/deploy.sh`, server setup docs, and systemd unit installation
- Test: bootstrap/recovery tests

- [ ] Ensure setup installs OS packages, service units, routing, directories, ownership, and binaries from source.
- [ ] Add a guided secret re-entry step for required Identity, GitHub, and service credentials without writing values to logs.
- [ ] Make setup rerunnable and expose service-by-service readiness before the public hostname is activated.

### Task 6: Migrate without data loss

**Files:**
- Modify: USB retention/migration logic and recovery docs

- [ ] Stop publishing new full-host snapshots only after the state-only exporter is live.
- [ ] Produce and validate one new-format export.
- [ ] Copy and re-hash that exact generation on the owner USB.
- [ ] Verify a restore on the disposable host after it becomes available.
- [ ] Only after owner review, retire the legacy archive generation set and old automatic sync machinery.

### Task 7: Verify and publish

- [ ] Run the focused archive, restore, USB, and Rust route test suites.
- [ ] Run the composed Platform test recipe and Windows USB recovery job.
- [ ] Inspect archive members and manifest flags without printing payloads.
- [ ] Deploy through the ordered approval queue and verify a live restore-status response and public route health.
