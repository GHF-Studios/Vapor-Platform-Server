# Per-service Resource Telemetry Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Show bounded per-service CPU, memory, and block-I/O usage for the selected Vapor host.

**Architecture:** Sample exact cgroup-v2 files for a fixed service allowlist from the existing Platform sampler and Linux node agent. Persist only the latest bounded remote snapshot, expose it through the authenticated fleet API, and render it in the existing Services view.

**Tech Stack:** Rust, serde, SQLite/SQLx, Linux cgroup v2, existing Axum admin API and inline UI.

**Spec:** `docs/superpowers/specs/2026-11-09-service-resource-telemetry-design.md`

## Global Constraints

- Service IDs and cgroup unit paths are fixed in code; no client-provided unit names or command execution.
- At most 10 services and 1 KiB per cgroup file are sampled.
- Missing, stale, malformed, reset, or unsupported measurements remain unavailable, never zero.
- Reports contain usage counters only; telemetry credentials still cannot execute operations.
- Remote storage is latest-snapshot only; no history is added in this slice.
- Root-only APIs stay private and `no-store`; the UI never interprets telemetry as markup.

## Review Focus

- Missing systemd/cgroup entries could be misreported as zero usage; test absence as unavailable.
- A cgroup counter reset could create an extreme CPU rate; test reset behavior.
- Malformed or oversized cgroup files could cause unbounded work or panic; test rejection and limits.
- Unknown service IDs could permit arbitrary path reads; test strict allowlisting.
- A stale node report could look live in the UI; test freshness and explicit age.

---

### Task 1: Bounded cgroup-v2 reader

**Files:** Create `src/service_telemetry.rs`; modify `src/main.rs` and `src/bin/vapor_node_agent.rs` module declarations; test in `src/service_telemetry.rs`.

**Interface:** `read_service_snapshot(root: &Path, previous: Option<&[ServiceMetrics]>, elapsed: Duration) -> Vec<ServiceMetrics>`. Map fixed service IDs to exact units. Parse `cpu.stat`, `memory.current`, and `io.stat`; derive interval CPU millicores only from monotonic counters and positive elapsed time.

- [x] Write failing parser tests for valid values, malformed/oversized content, unavailable units, reset counters, and unknown IDs.
- [x] Run focused tests and confirm failures are for missing behavior.
- [x] Implement fixed allowlist and bounded file reads with unavailable fields on missing/invalid source data.
- [x] Run focused tests; verify CPU calculations use 1000 mCPU per fully busy core.

### Task 2: Versioned telemetry ingest and cached host snapshot

**Files:** `src/bin/vapor_node_agent.rs`, `src/node_fleet.rs`, `src/server_fleet.rs`.

- [x] Add failing tests for optional service-family parsing, range/count validation, persistence on accepted reports, and rejection without overwrite.
- [x] Run focused tests and confirm failures.
- [x] Include per-service metrics in the existing sampler/report without a new polling loop.
- [x] Add `services_json` to node inventory and return the cached bounded snapshot in the root-only fleet response.
- [x] Verify schema-1/2 and unknown-version agents are rejected; schema 3 requires the complete report shape.

### Task 3: Compact Services view

**Files:** `src/admin.rs`.

- [x] Add failing page tests for the table, stale/unavailable wording, and the absence of action controls.
- [x] Implement text-only CPU/memory/read/write rows and sample age in the existing server Services panel.
- [x] Parse the embedded script and run the focused Server page/API tests.

### Task 4: Verify and publish

- [x] Run the Platform test recipe (all service tests and Python recovery checks), Windows recovery suite, and embedded-JS parse. Linux node-agent packaging remains a CI verification because this workstation has no Linux C compiler.
- [ ] Review schema storage and the no-history boundary; update service/domain docs.
- [ ] Commit and push a coherent slice; wait for Linux and Windows CI.
- [ ] Do not deploy until the change is on the approved deployment path; verify a real Linux sample on a test host first.
