# Remote Server Telemetry Implementation Plan

**Goal:** Enroll remote Linux nodes and display bounded host telemetry in the private Server Fleet page.

**Architecture:** Platform owns enrollment, per-node credential digests, report validation, and inventory state in its existing SQLite database. A standalone Linux reporter sends typed metrics over HTTPS; its credential only permits telemetry submission.

**Spec:** `docs/superpowers/specs/2026-11-09-remote-server-telemetry-design.md`

## Constraints

- No SSH polling, web SSH, arbitrary remote commands, or service control.
- Platform alone owns fleet inventory and enrollment.
- Persist credential digests only; return the reporter secret once.
- Roles are descriptive labels, not authority.
- Preserve honest local-only fleet state until a node reports.

## Tasks

### 1. Persistent inventory and credential lifecycle

**Files:** `src/node_fleet.rs`, `src/main.rs`, `src/admin.rs`, `src/server_fleet.rs`.

- [x] Initialize idempotent tables for nodes and append-only create/revoke audit events.
- [x] Add root-only create/list/revoke routes with label and role validation, actor attribution, and no-store responses.
- [x] Generate a node-scoped secret with OS-backed randomness, persist only its SHA-256 digest, and return it only at creation.
- [x] Add enrolled nodes to the private fleet API without replacing local host sampling.

### 2. Bounded telemetry ingestion

**Files:** `src/node_fleet.rs`, `src/server_fleet.rs`.

- [x] Add credential-only telemetry endpoint with a 4 KiB body limit.
- [x] Validate metric ranges and sample-time bounds.
- [x] Atomically enforce a 10-second receipt interval and strictly increasing sample timestamps.
- [x] Store Platform receipt time and report online/stale/revoked states truthfully.
- [x] Reject malformed, oversized, revoked, stale, replayed, or out-of-order reports without overwriting the last accepted sample.

### 3. Linux reporter

**Files:** `src/bin/vapor_node_agent.rs`, `deploy/node-agent/README.md`, `deploy/node-agent/vapor-node-agent.service`, `Cargo.toml`.

- [x] Read root-owned, group-readable configuration and run as a dedicated unprivileged service account.
- [x] Require a bare HTTPS origin and refuse redirects.
- [x] Sample CPU deltas, memory, uptime, and load from procfs.
- [x] Add one-shot and recurring modes with bounded timeouts.
- [x] Document setup and minimum permissions.

### 4. Fleet controls and state clarity

**Files:** `src/admin.rs`, `src/server_fleet.rs`.

- [x] Add compact enrollment controls, one-time credential display, copy and revoke actions.
- [x] Keep the credential out of browser storage and clear it when its view closes.
- [x] Show remote sample age and online, stale, never-reported, and revoked states.
- [x] Keep service health and deployment operations separate.

### 5. Verification and rollout

- [x] `cargo check --bin vapor_platform_server --bin vapor_node_agent` on Windows.
- [x] Composed CI passed on Ubuntu and Windows; Ubuntu compiled and ran `deploy/scripts/test.sh` successfully (main run `42651713649`).
- [ ] Local Linux cross-check remains unavailable because this workstation lacks `x86_64-linux-gnu-gcc`; native Ubuntu CI is the authoritative Linux build.
- [ ] Install the reporter only on a disposable worker after verifying endpoint behavior.
- [ ] Deploy Platform, confirm the local VPS remains visible, enroll one disposable node, check fresh/stale/revoked behavior, then revoke it.

## Review focus

- Reporter secrets never appear in logs, database reads, or list responses.
- Revocation immediately rejects new reports.
- Invalid or replayed samples cannot overwrite accepted state.
- Fleet/browser routes remain root-only while telemetry remains credential-only.
