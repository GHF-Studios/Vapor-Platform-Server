# Host Network Telemetry Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show truthful, private per-server network traffic and recent aggregate trends for the Platform host and authenticated Linux nodes.

**Architecture:** A shared bounded Linux `/proc` parser supplies the Platform sampler and node agent. Node reports carry cumulative per-interface counters and TCP state counts; Platform persists only aggregate samples in its already-backed-up database and exposes them through the existing root-authorized Server API. The Server control room renders a compact Traffic view. Detailed peer/process inspection and packet capture remain explicit later slices; neither enters periodic telemetry.

**Tech Stack:** Rust, serde, Axum, SQLx/SQLite, inline admin HTML/JavaScript, Linux `/proc`.

**Spec:** `docs/superpowers/specs/2026-11-09-server-observability-workbench-design.md`

## Global Constraints

- Diagnostics telemetry is Platform-owned operational data, not player Diagnostics uploads.
- Keep periodic telemetry to bounded interface counters and aggregate TCP state counts; do not collect peers, payloads, process names, or hostnames.
- Support old schema-version 1 agent reports without network fields while new agents send schema version 2.
- Enforce a 16-interface maximum, at most 10 seconds of timestamp skew, the existing 30-second report freshness/rate limits, and seven-day sample retention.
- Keep per-node sample storage bounded at 20,160 rows and downsample history responses to at most 1,008 points.
- Return private, no-store responses only after existing root authorization.
- Missing or stale data must remain unavailable/stale, never be rendered as a zero measurement.
- Do not alter user-submitted Diagnostics service behavior.

## Review Focus

- Malformed, oversized, duplicate, or counter-reset proc values never panic or produce negative rates.
- Legacy schema-version 1 agents remain accepted and show network data as unavailable.
- Revoked credentials, stale/out-of-order samples, and cross-node IDs cannot write history.
- History queries cannot exceed the fixed time-window, sample-count, or response-size limits.
- UI text and charts must render with DOM text APIs and never interpret telemetry as markup.

---

### Task 1: Shared bounded Linux network parser

**Files:**
- Create: `src/network_telemetry.rs`
- Modify: `src/main.rs`
- Modify: `src/bin/vapor_node_agent.rs`
- Test: inline unit tests in `src/network_telemetry.rs`

**Interfaces:**
- `NetworkSnapshot { interfaces: Vec<InterfaceCounters>, tcp_states: TcpStateCounts }`
- `read_network_snapshot() -> Option<NetworkSnapshot>` reads `/proc/net/dev`, `/proc/net/tcp`, and `/proc/net/tcp6`.
- `valid_network_snapshot(&NetworkSnapshot) -> bool` checks fixed bounds and monotonic counter types.

- [x] Add fixture tests for interface field ordering, IPv4/IPv6 TCP states, loopback, malformed rows, and the 16-interface cap.
- [x] Run the focused parser tests and confirm malformed rows are rejected without panic.
- [x] Implement the shared parser and bounded serialized data types; include the module in both Platform and the node-agent binary.
- [x] Run the focused parser tests and the Windows Rust build/test check. Linux cross-target check is still blocked locally by missing `x86_64-linux-gnu-gcc`; Linux CI must verify that target.

### Task 2: Versioned node-agent telemetry report

**Files:**
- Modify: `src/bin/vapor_node_agent.rs`
- Test: inline unit tests in the agent module

**Interfaces:**
- Schema 2 adds `network: NetworkSnapshot` to the existing report.
- The agent samples cumulative counters every existing 30-second interval; it does not calculate rates or collect peers.

- [x] Test schema 2 network serialization and the unchanged legacy schema 1 shape.
- [x] Make missing proc sources omit the network family rather than fabricate zeros.
- [x] Implement schema 2 reporting through the shared parser.
- [ ] Build the Linux node-agent package and verify its archive contains the updated version.

### Task 3: Validated ingest and bounded persisted history

**Files:**
- Modify: `src/node_fleet.rs`
- Modify: `src/server_fleet.rs`
- Test: `src/node_fleet.rs` telemetry API tests

**Interfaces:**
- `TelemetryReport.network` is optional for schema 1 and required/validated for schema 2.
- `GET /api/admin/server-nodes/{id}/network?window=1h|6h|24h|7d` returns fixed-schema aggregate samples.
- `record_network_sample(node_id, sampled_at_unix, snapshot)` stores in `platform_server_network_samples`.

- [x] Test schema 1 compatibility and valid schema 2 ingest; existing tests cover stale time, cross-node IDs, and revoked credentials for the shared authenticated endpoint.
- [ ] Add explicit tests for malformed network metrics, seven-day pruning, and each allowed query window/sample ceiling.
- [x] Implement idempotent SQLite migration, authenticated ingest, local sampler recording, and fixed-window query route.
- [x] Add server-side rate derivation in the view; first samples and counter resets render unavailable rather than negative.
- [x] Verify by inspection that Platform state export includes `platform.sqlite3`; no separate backup artifact is needed.

### Task 4: Server control-room Traffic view

**Files:**
- Modify: `src/admin.rs`
- Test: existing server fleet page/API rendering tests

**Interfaces:**
- Add a `Diagnostics` tab with a `Traffic` view for the currently selected server.
- Show sample time/freshness, RX/TX rates, per-interface counters, errors/drops, TCP states, and 1h/6h/24h/7d selection.

- [x] Test the rendered tab, range choices, stale/unavailable states, and safe text construction.
- [x] Implement a compact text-first view and small native canvas trend lines; no external chart dependency.
- [x] Keep refresh user-triggered and prevent overlapping requests; add no fast polling loop.
- [ ] Parse the embedded JavaScript and run the focused page tests.

### Task 5: Documentation, review, publish, and deploy

**Files:**
- Modify: `deploy/node-agent/README.md`
- Modify: `docs/server-domain-boundaries.md`
- Modify: `docs/deployment-status.md`

- [x] Document collected fields, data freshness, retention, privacy boundary, and the package rebuild requirement.
- [ ] Review every changed line, migration path, response size, and state export behavior.
- [ ] Run the focused Rust and Python verification plus the repository deployment test recipe.
- [ ] Build the Linux agent package, commit, push, wait for required CI, then submit the exact-main deploy request.
- [ ] Verify deployed health, protected Traffic API behavior, and the first live local-host sample before reporting completion.

## Follow-on Work Not Included in This Slice

- On-demand connection snapshots with local/remote endpoints and process attribution.
- SSH/service security-event summaries backed by bounded root-owned journal reads.
- Explicit, reasoned, expiring packet captures in verified volatile storage.
- Bootstrap and enroll the new Ubuntu worker after host-key verification and root-password rotation.
