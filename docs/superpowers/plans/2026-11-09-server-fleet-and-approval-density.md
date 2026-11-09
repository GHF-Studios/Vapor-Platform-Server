# Server fleet and approval density implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a compact, accurate deployment decision surface and a private first-node fleet view backed by cached host metrics.

**Architecture:** Platform owns the fleet inventory surface and caches metrics sampled from its own host. Future hosts will report outward through a narrow node agent; the dashboard will not SSH-poll nodes or expose arbitrary shell operations. Deployment request policy stays in its current API and worker owners.

**Tech Stack:** Rust, Axum, Tokio, embedded HTML/CSS/JavaScript, existing Identity root-session authorization.

**Spec:** `docs/superpowers/specs/2026-11-09-server-fleet-and-approval-density-design.md`

## Global Constraints

- Keep server inventory/operations in Platform; do not move service business logic into the root repository.
- Keep Hosting for service health and Deployments for deployment lifecycle.
- Do not collect public/player telemetry or show unenrolled nodes as real.
- Do not introduce arbitrary remote shell or SSH polling.
- Preserve exact current-main, target validation, and queue-order deployment guards.

## Review Focus

- Unauthenticated/non-root fleet requests must not disclose machine metrics.
- Unsupported hosts and unreadable metric sources must report unavailable, not plausible zeros.
- Stale telemetry must not appear current.
- Compact approval actions must still confirm the exact SHA and target and retain ordering/currentness checks.
- The sampler must do bounded work outside request handling.

---

### Task 1: Cached local node telemetry

**Files:** Create `src/server_fleet.rs`; modify `src/main.rs` and `src/admin.rs`; tests in `src/server_fleet.rs` and `src/admin.rs`.

- [x] Define serializable node/metric snapshot types and a bounded sampler for Linux CPU, memory, uptime, and load.
- [x] Add tests for `/proc` parsing, first-sample CPU unknown, delta CPU percentage, and unavailable input behavior.
- [x] Start one background sampler and serve its cached snapshot behind root authorization.

### Task 2: Private Server Fleet page

**Files:** Modify `src/admin.rs` and `docs/server-domain-boundaries.md`.

- [x] Add `/admin/servers` and `/api/admin/servers`.
- [x] Render the current VPS as one node with role, metrics, and explicit observation time.
- [x] Show local-only scope and the not-yet-enabled remote enrollment state without inventing nodes.
- [x] Keep service-health status in Hosting.

### Task 3: Compact deployment approvals

**Files:** Modify `src/admin.rs`; tests in `src/admin.rs`.

- [x] Render repository/branch, deployment target, short exact commit, CI link, and current/order status in a dense decision row.
- [x] Move requester details and full submodule pins behind a disclosure control.
- [x] Replace duplicated tall checkbox forms with a confirmation dialog that names the exact SHA and target.
- [x] Retain stale revision hiding, ordering disablement, and exact request versions.

### Task 4: Verify and publish

- [x] Run Rust tests, deployment queue tests, embedded JavaScript syntax validation, and `git diff --check`.
- [x] Review the resulting diff against the spec and current server boundary docs.
- [ ] Commit and push the coherent slice to `main`; deploy through the exact tested revision request path.
- [ ] Verify the live Server Fleet page, telemetry freshness, approval counts, and deployment state.
