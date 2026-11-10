# Host-First Server Operations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Servers workbench represent general host administration on a single-host-first platform, while keeping Vapor services as independent processes.

**Architecture:** Remove the unsupported all-services-in-one-binary runtime branch and role assumptions from physical-host connection setup. Keep the shared Platform gateway, independent service processes, current one-host operating model, and a bounded generic process inventory on the host workbench.

**Tech Stack:** Rust/Axum/SQLite, Python and Bash deployment/recovery tooling, systemd, GitHub Actions, embedded admin JavaScript.

**Spec:** `docs/superpowers/specs/2026-11-09-host-first-server-operations-design.md`

## Global Constraints

- The production deployment remains one physical host until an operator deliberately provisions a replacement or test host.
- Each Vapor service remains a separate process and fixed systemd unit.
- Platform remains the shared admin gateway and deployment authority.
- Never collect process arguments, environments, executable paths, packet payloads, or peer addresses.
- Never reset Identity, deployment approvals, SSH grants, or server-operation records as part of the host inventory schema reset.
- Preserve unrelated dirty and untracked worktree files.

## Review Focus

- A `monolith` request or recovery bundle must fail before any mutation.
- A service-scoped deployment must remain available in the sole `services` runtime.
- Recreating host-inventory tables must not drop other tables in the Platform database.
- Process snapshots must enforce per-row and whole-payload bounds and reject duplicate/invalid PIDs.
- UI must show stale/unavailable process data as such and must never render command arguments or raw executable paths.

---

### Task 1: Make independent services the only runtime

**Files:** `src/main.rs`, `Cargo.toml`, `deploy/scripts/lib.sh`, `deploy/scripts/deploy.sh`, `deploy/scripts/health-check.sh`, `deploy/scripts/export-state.sh`, systemd units/installers, queue/request/runtime validation, recovery metadata/restore scripts, tests and current deployment docs.

- [x] Update request, queue, recovery, and deployment checks to reject the retired monolith topology.
- [x] Remove the conditional all-service router from `src/main.rs` and its service-crate root dependencies.
- [x] Make independent services the only deploy, health, install, and restore path.
- [x] Install the shared Platform gateway as the Platform systemd unit and remove the old monolith unit source.
- [x] Keep the `services` marker only in existing contracts that still require a topology value; do not mutate auth/deploy databases.
- [x] Run focused Rust/Python/shell tests, then the full repository test recipe on the local Windows host. Linux CI remains a separate target.

### Task 2: Remove guessed host roles

**Files:** `src/node_fleet.rs`, `src/server_fleet.rs`, `src/admin.rs`, host connection tests.

- [x] Remove the role selector and role from new host-connection requests and public responses.
- [x] Remove the still-internal role columns/metadata from the host tables through a scoped schema reset; preserve unrelated Platform tables.
- [x] Preserve stable host-place replacement and version checks using only the label and host-place identity.
- [x] Run the focused connection inventory tests after the schema reset.
- [x] Run focused node-fleet and admin tests.

### Task 3: Add general process visibility

**Files:** create shared `src/process_snapshot.rs`; modify `src/bin/vapor_node_agent.rs`, `src/node_fleet.rs`, `src/server_fleet.rs`, and `src/admin.rs`; add Rust tests and admin response tests.

- [x] Add parser tests for `/proc/<pid>/stat` command names containing spaces/parentheses, UID/memory parsing, truncation, and output bounds.
- [x] Add a schema assertion that process output excludes argv, executable paths, and environment values.
- [x] Add a root-authorized, bounded local-host process snapshot with PID, state, CPU rate, RSS, UID, and sample time.
- [x] Add validated process snapshots to connected remote-host reports without changing the network/service-resource schemas.
- [x] Render a compact process table with sample age and unavailable states.
- [x] Run focused tests, embedded-JavaScript syntax checks, and the full repository test recipe on the local Windows host. Linux CI remains a separate target.

### Task 4: Review and publish

- [ ] Review the complete diff against the design and inspect deployment/recovery paths for leftover executable-monolith behavior.
- [ ] Run CI for Linux and Windows recovery tooling.
- [ ] Commit the coherent changes, push, and deploy only through the approved request path; then verify active service units and health checks.
