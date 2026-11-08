# Vapor Platform Recovery and USB Copy Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Produce consistent hourly full-platform recovery archives, copy them to the owner's unencrypted 128 GB USB drive when available, and make a fresh-host restore path explicit.

**Architecture:** Platform coordinates one archive containing stopped/quiesced service state, server configuration/credentials, selected host routing configuration, and exact source/submodule pins. A Windows PowerShell task pulls new completed archives over the owner's existing root SSH key, verifies them before retaining them, and leaves VPS generations available for catch-up. Build outputs and target snapshots remain rebuildable and excluded. A second VPS may later receive a verified replica but is not required.

**Tech Stack:** Existing Bash deployment scripts, Python stdlib tar validator, systemd timer, Windows PowerShell and OpenSSH client.

**Spec:** `docs/superpowers/specs/2026-11-06-vapor-platform-operations-and-recovery-redesign.md`

## Global Constraints

- Keep secrets out of Git and reports; the recovery archive itself intentionally includes secrets in plaintext by owner decision.
- Keep Platform as recovery coordinator and preserve service-owned state semantics.
- Do not make the worker/secondary VPS a prerequisite for backup, local build, deployment, or recovery.
- Preserve the owner root SSH key as a separate break-glass path; no HSM or custom lockdown protocol.
- Never claim full recovery until a clean-host restore has been performed.
- Preserve unrelated untracked scratch and VPS files.

## Review Focus

- Export during concurrent writes must quiesce every active service that writes included state; test monolith and services mode.
- A failed or interrupted archive must never replace the last complete archive or its latest pointer.
- Bundle validation must reject traversal, links/special files, duplicates, missing config, invalid manifests, and bad checksums without printing contents.
- Restore failure must put old state and configuration back before restarting previously active services.
- USB absence, SSH interruption, hash mismatch, duplicate generation, or insufficient space must leave prior verified copies intact and surface a concise failure.

---

## File Map

- `deploy/scripts/export-state.sh`: coordinated archive creation, metadata, validation, checksums, atomic publication, and VPS retention.
- `deploy/scripts/state-bundle-validate.py`: versioned structure/content contract for restore and local copy verification.
- `deploy/scripts/restore-state.sh`: safe preflight, replacement, rollback, and service restart behavior.
- `deploy/scripts/backup-to-usb.ps1`: Windows pull, integrity verification, catch-up, capacity-aware retention, and local sync report.
- `deploy/systemd/vapor-state-export.service`: invoke the current exporter and keep backup staging root-owned.
- `deploy/README.md`, `docs/operations-evidence-and-recovery-contract.md`, `docs/deployment-status.md`: describe current behavior and honest recovery limits.
- `deploy/scripts/README.md` is not present; do not create it just for this change.

## Task 1: Correct the recovery bundle contract

**Files:**
- Modify: `docs/operations-evidence-and-recovery-contract.md`
- Modify: `deploy/README.md`
- Modify: `docs/deployment-status.md`

- [x] State that schema v2 bundles include `/var/lib/vapor-server` except rebuildable Build Cache targets, `/etc/vapor-server`, `/etc/caddy/Caddyfile`, source root revision and recursive submodule pins.
- [x] State that config/credential values are included only inside the intentionally unencrypted archive and are never written to logs or manifests.
- [x] Distinguish the VPS-local hourly export from a verified off-host USB copy; do not describe the current setup as fully recoverable until the sync and restore are proven.
- [x] Record that root SSH private key stays separate from the USB recovery archive.

## Task 2: Make VPS export consistent and atomic

**Files:**
- Modify: `deploy/scripts/export-state.sh`
- Modify: `deploy/scripts/state-bundle-validate.py`

- [x] Stop all configured state writers in services mode, including Homepage, Docs, Identity, Diagnostics, Registry, Artifact, Build Cache, and Platform; in monolith mode stop the composed Platform unit. Restart only units that were active before the export.
- [x] Copy the state tree while stopped, omitting only `build-cache/targets` and explicitly rebuildable candidate scratch; include config and selected Caddy host config when present.
- [x] Record schema v2, UTC timestamp, source root SHA, recursive submodule pins, runtime mode, included categories, plaintext-secret marker, and archive size/hash metadata without secret values.
- [x] Write to a temporary file under the backup filesystem, validate its structure, calculate SHA-256, then atomically rename archive, checksum, and latest-generation pointer into place.
- [x] Keep timestamped generations for catch-up; prune only complete validated archives after retaining the configured count and never remove the current latest generation.
- [x] Extend validation to require all schema-v2 categories, validate the pointer/checksum, and retain strict archive path/type/duplicate protections.

## Task 3: Make restore transactional

**Files:**
- Modify: `deploy/scripts/restore-state.sh`
- Modify: `deploy/scripts/state-bundle-validate.py`

- [x] Validate archive and checksum before stopping services.
- [x] Stage restored state/config/host files separately; preserve current state/config under a timestamped pre-restore location.
- [x] Install staged state and configuration with expected ownership/modes, reload systemd/Caddy only after file replacement, and run local readiness checks.
- [x] On any pre-health or health failure, restore the preserved state/config and restart the previously active services using the pre-restore configuration.
- [x] Record outcome and rollback result without paths, archive member names, error bodies, or secret values.

## Task 4: Add Windows USB catch-up

**Files:**
- Create: `deploy/scripts/backup-to-usb.ps1`
- Modify: `deploy/README.md`

- [x] Accept explicit host, USB destination, and existing SSH identity file parameters; default to no drive letter or key assumptions.
- [x] Fetch only the root-readable latest-generation index and list of immutable completed generations over OpenSSH/SCP.
- [x] Download unseen generations into a temporary destination folder, verify the declared SHA-256, atomically rename only after verification, and write a local last-success report on the USB.
- [x] Resume missed generations on later runs; detect missing USB, missing tools/key, SSH failure, hash mismatch, and low space without deleting the last known-good copy.
- [x] Retain generations by a configurable maximum byte budget, deleting oldest verified copies only after a newer verified generation is safely present.
- [x] Document a Task Scheduler setup that runs hourly while the USB is mounted, and a manual one-command catch-up path.
- [x] Serialize scheduled/manual syncs per destination before cleanup of partial files; keep test-only SSH/SCP adapters out of the normal command interface.

## Task 5: Recovery readiness and deployment handoff

**Files:**
- Modify: `docs/operations-evidence-and-recovery-contract.md`
- Modify: `docs/superpowers/specs/2026-11-06-vapor-platform-operations-and-recovery-redesign.md`

- [x] Describe the fresh Ubuntu Strato sequence: recover Strato account, provision host, install pinned source/submodules, restore config/state from USB, reauthorize invalid provider integrations, then expose public routes.
- [x] Describe incident recovery separately: preserve evidence and rotate provider/SSH credentials before restoring an image suspected of compromise.
- [x] Mark the second VPS as optional build/replica capacity; make local builds an explicit supported fallback and shared cache freshness a separate status.
- [x] Leave clean-host restore and USB-copy drills as uncompleted acceptance gates until the owner has the drive and second host available.

## Task 6: Exercise the Windows copy path without real infrastructure

**Files:**
- Create: `deploy/tests/test_backup_to_usb.ps1`
- Modify: `.github/workflows/ci.yml`

- [x] Use local JSON/archive fixtures and test-only command adapters; do not contact the VPS or require the owner's USB.
- [x] Cover initial copy and catch-up, latest-pointer/index mismatch, corrupt archive rejection while preserving an older verified copy, and same-destination overlap rejection.
- [x] Run the fixture suite in GitHub Actions on a native Windows runner.
- [ ] Register the owner-held scheduled task and verify a real copy on the owner's USB.
- [ ] Run a clean-host restore from a USB generation.

## Task 7: Stage fresh-host runtime before restoring state

**Files:**
- Modify: `deploy/scripts/recover-fresh-host.sh`
- Modify: `deploy/scripts/deploy.sh`
- Modify: the unit/Caddy/state-export installers
- Modify: `deploy/scripts/bootstrap-ubuntu.sh`
- Create: `deploy/tests/test_recovery_activation_order.py`

- [x] Suppress package daemon starts during fresh-host recovery setup without overwriting an existing host `policy-rc.d`.
- [x] Build and install the pinned runtime and operational helpers without enabling service, Caddy, or state-export units.
- [x] Restore the saved state/configuration before `restore-state.sh` activates the restored units and runs readiness checks.
- [x] Add regression checks for staging-before-restore and disabled pre-restore activation.
- [ ] Run a clean-host restore drill to validate real systemd/package behavior.

## Follow-up Task 8: Carry an exact recovery entrypoint on the USB

- [x] Export the fresh-host coordinator, its pre-checkout metadata reader,
  a concise operator README, and a SHA-256 list from the same pinned source
  commit as the archive.
- [x] Validate kit completeness and file hashes while continuing to accept
  legacy schema-v2 archives that predate the optional kit.
- [x] Extract the kit from the latest content-verified archive into a
  root-commit-specific USB directory and atomically publish its pointer.
- [x] Cover normal extraction, repeated sync, invalid kit hashes, and legacy
  archives without a kit in the Windows fixture suite.
- [ ] Verify the kit on the owner's removable drive and run a clean-host
  recovery drill.

## Follow-up Task 9: Gate recovery on configured providers only

- [x] Keep Steam identity and the owner dashboard required for recovery.
- [x] Require GitHub browser readiness only if either GitHub OAuth credential
  is configured; fail closed if Identity omits the GitHub configuration flags.
- [x] Add regression cases for unconfigured GitHub, configured-but-unready
  GitHub, missing Steam, and unknown GitHub configuration.
- [x] Make fresh-host instructions refer to configured provider checks rather
  than requiring GitHub sign-in for every installation.

## Execution Notes

- The user authorized the unencrypted archive contents and ordinary root SSH access. This does not authorize deleting historical server archives, changing root keys, deploying, or running a destructive restore.
- A separate second-node implementation plan is required after the owner chooses a concrete Strato offering and shares its non-secret capacity/spec sheet.
- The shared web admin remains a separate implementation plan; its UI can report server export state, while USB sync status initially remains local until a safe receipt flow is designed.

## Source implementation status (not deployed)

- The exporter now rejects dirty source/submodule checkouts, captures host OS,
  architecture, exact Rust/Cargo versions and host triple, quiesces all eight state-writing services, and validates
  the schema-v2 archive before publishing its digest/index/latest pointer.
- The restore path checks the archive digest and requires a clean checkout at
  the exact recorded root and submodule revisions before it mutates host state.
  It preserves prior state/configuration and reports rollback completion in
  stderr/systemd journal output.
- New exports include the exact recovery entrypoint, metadata reader, operator
  README, and embedded SHA-256 list. The Windows sync extracts these into a
  root-commit-specific folder only after verifying their hashes and writes an
  atomic current-kit pointer. Older archives without a kit remain copyable.
- The Windows pull/task scripts exist, but no scheduled task has been registered
  on the owner's PC and no USB copy has been exercised.
- The Windows fixture suite uses fake SSH/SCP commands and passed locally on
  PowerShell 7.6.9; CI is configured to run it on `windows-latest`. This
  verifies the copy logic and overlap guard without proving removable-media
  behavior or a real network transfer.
- The pull path follows the VPS `latest.json` pointer and checks it against the
  generation index. It re-hashes every indexed archive each sync, restores a
  damaged indexed copy from the VPS before replacing it, and verifies that
  download. Unknown/manual archives are preserved and reported as unverified;
  capacity pruning only follows a successful full indexed-archive audit. The
  source behavior has been exercised against the owner's drive, but not yet in
  a clean-host restore drill.
- Fresh-host recovery now stages binaries and units with application services
  disabled until the validated state/configuration import. This is
  source-tested only; package scripts and systemd ordering still need a clean
  Ubuntu recovery drill.
- Compiler values in the bundle are observations from export time, not proven
  build provenance for the running executables. The clean-host coordinator
  reproduces those exact observed versions, but this repository does not own
  Vapor Client's ecosystem-wide canonical toolchain lock; that identity must be
  imported from the client before the optional worker path is implemented.
- The Platform gateway now checks its fixed owner-admin routes through local
  Caddy during deployment health checks. This has not been deployed or run.
- State-bundle tests were updated to construct schema-v2 fixtures and run on
  Windows through Git Bash. Linux symlink publication tests require a native
  symlink-capable host; they are explicitly skipped on Windows when the caller
  lacks symlink privileges.
- The restore script now tolerates absent old Vapor units on a genuinely empty
  host while treating unreadable/unexpected unit states as failures. It still
  does not provision Ubuntu, checkout source, compile service binaries, or
  validate a fresh-host restore.
- A guarded `recover-fresh-host.sh` coordinator now composes prerequisite
  installation, exact source/submodule checkout, recorded-toolchain rebuild,
  normal deployment, and state restore. It leaves a recovery marker pending
  until the owner completes Steam/GitHub browser checks and explicitly confirms
  the owner dashboard; a failed post-deploy run disables and stops the partial
  stack.
  It only accepts an empty supported Ubuntu x86_64 host and leaves partial
  state intact for diagnosis on failure. No real clean-host drill has run yet.
- The Rust/Cargo versions in an archive are observations, not build provenance;
  the fresh-host coordinator uses them as explicit rebuild inputs and verifies
  the installed versions before compiling. The optional remote compiler cache
  remains a separate acceleration design and is not required for recovery.
