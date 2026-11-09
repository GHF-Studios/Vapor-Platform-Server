# Owner-operated server workbench implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the two-host Vapor setup observable, explicitly operable, and rebuildable through typed, recorded owner-run workflows.

**Architecture:** Keep inventory and coordination in Platform, telemetry and typed execution on each host, and service state in its owning service. Each action is explicitly started by the root operator and persisted before dispatch. Migration is a resumable runbook with human checkpoints; it never autoscales or allocates hardware.

**Tech Stack:** Rust, Axum, SQLite, existing Identity root-session authorization, local root-owned Unix-socket helper, bounded Linux node agent, systemd, existing per-service export/import/deploy contracts.

**Spec:** `docs/superpowers/specs/2026-11-09-owner-operated-server-workbench-design.md`

## Global Constraints

- No Strato API integration for VPS creation, resizing, or deletion.
- No metric-triggered lifecycle, automatic job placement, autoscaling, or unattended migration.
- No shell/command/path/unit-name text from the browser; operation payloads use enums and allowlists.
- Telemetry credentials do not authorize typed operations.
- Root Identity authorization and same-origin mutation checks remain mandatory.
- Platform coordinates service-owned export/import/deploy contracts and never reads service databases directly.
- Every operation and migration step is durably recorded before execution; uncertain results are never blindly retried.
- Secrets remain root-owned and are absent from browser responses, operation records, and logs.

## Review Focus

- Duplicate or stale browser submissions cannot execute the same operation twice; test compare-and-set version conflict.
- Unknown operation variants and unrecognized service IDs are rejected before the root helper runs; test both.
- Helper or HTTP failure after the record becomes running leaves an honest `outcome_unknown` state; test recovery reconciliation.
- A host reboot can interrupt the request; test boot-ID based completion instead of returning false success.
- Removing/replacing a host preserves operation and migration history while revoking its node credential; test historical retention.

---

### Task 1: Align fleet intent and visible terminology

**Files:** `src/admin.rs`, `src/node_fleet.rs`, `docs/server-domain-boundaries.md`, the two existing fleet/telemetry specs and plans.

- [x] State in the Servers page that nodes are manually managed and registration does not provision, remove, schedule, or grant service control.
- [x] Label purpose as informational and say jobs are never assigned from that value.
- [ ] Review every node-role use; retain a role only where it affects an explicit profile/apply operation, otherwise make it a plain label.
- [ ] Update fleet acceptance tests to assert the manual-operation/no-scheduler language.
- [ ] Run the focused Server Fleet test and `git diff --check`.

### Task 2: Durable typed operation ledger

**Files:** Create `src/server_operations.rs`; modify `src/main.rs`, `src/admin.rs`, `src/operation_history.rs` only for routing/history composition.

- [x] Add Platform-owned `platform_server_operations` and append-only `platform_server_operation_events` tables with target node ID, enum action, canonical arguments, actor profile ID, state/version, timestamps, and bounded outcome. SQLite triggers reject mutation/deletion of event rows.
- [x] Expose root-only, no-store list/detail/create routes. Validate exact Origin on mutations and gate every target to the local executor.
- [x] Persist intent before dispatch. Record `accepted`, `running`, `succeeded`, `failed`, and `outcome_unknown` transitions with event rows and compare-and-set state transitions.
- [x] Reconcile interrupted `running` operations on startup as `outcome_unknown`; never replay them automatically. A matching idempotency-key retry can resume only an operation still in `accepted`.
- [ ] Test authorization, invalid action/target inputs, transition compare-and-set, append-only event order, bounded results, and interrupted execution comprehensively.

### Task 3: Core-host executor actions

**Files:** `src/bin/vapor_platform_ops.rs`, `src/platform_ops_client.rs`, `src/server_operations.rs`, `deploy/scripts/vapor-ssh-service-control` only if shared allowlist extraction is necessary.

- [x] Add enum-only `restart_service(service_id)` for the exact Vapor/Caddy unit allowlist. `reboot_host` is defined in the schema but not exposed until boot-ID reconciliation and high-impact confirmation are implemented.
- [x] Require a current root Identity session in the helper and a matching persisted operation ID/action/target before execution.
- [x] Persist service-restart helper results directly before returning to the web process. Platform self-restart remains intentionally unavailable because its recovery-after-restart behavior is not yet verified.
- [ ] For reboot, record the pre-action Linux boot ID and mark success only after a different boot ID is observed; leave a bounded timeout as failed/unknown with no automatic retry.
- [ ] Test JSON rejection of arbitrary fields, unit-name injection, unsupported host targets, helper mismatch, and boot-ID reconciliation.

### Task 4: Servers page operation surface

**Files:** `src/admin.rs`, `src/server_operations.rs`.

- [x] Add compact controls for restarting an allowlisted service on the local Platform host. Remote nodes remain telemetry-only.
- [x] Confirm the exact service before restart and disable duplicate submissions while the request is active.
- [x] Display the operation result and a compact recent-action list. A dedicated operation detail route exists; linking from each row remains to do.
- [ ] Keep remote nodes telemetry-only until a bounded remote executor is installed; no UI control is inferred from an active telemetry credential.
- [ ] Parse embedded JavaScript and test route/page rendering and action-state wording.

### Task 5: Typed server profiles

**Files:** new Platform profile module/API, recovery export/import manifests, setup/bootstrap tooling, Servers page.

- [ ] Define typed, non-secret profile fields from the actual current VPS configuration and systemd/Caddy setup.
- [ ] Store profile revisions in Platform-owned recoverable state. Preview a redacted diff before explicit apply.
- [ ] Keep secrets in root-owned host files and the owner-held recovery set; never expose or round-trip secret values through the browser.
- [ ] Apply profiles through the typed executor and record their revision/diff in the operation ledger.
- [ ] Verify exported/restored profiles on a clean local fixture before live migration.

### Task 6: Two-host rebuild and migration runbook

**Files:** Platform migration module/API, node executor protocol, service-owner export/import contracts, Servers and Recovery UI, docs.

- [ ] Model a manually advanced workflow with stable logical host slots, source/target incarnations, chosen service set, pinned profile and state revisions, per-step results, and explicit continue/abort actions.
- [ ] Preflight capacity, service readiness, source exports, target profile, and current deployment revision before any move.
- [ ] Move only services whose owning repository exposes a valid export/import/deploy contract; preserve the local-build fallback when no worker is available.
- [ ] Verify target health and data before the operator retires the source host through Strato.
- [ ] Support moving responsibilities back through the same recorded sequence.
- [ ] Add operation/migration/profile state to whole-system export/import and USB state recovery; prove with a clean restore drill.

### Task 7: Integrate and roll out

- [ ] Run `deploy/scripts/test.sh` on Ubuntu CI and the complete Windows recovery test set.
- [ ] Review the diff against the spec, service-domain boundaries, and the mounted recovery contract.
- [ ] Commit/push a coherent slice and deploy one exact revision at a time.
- [ ] Validate the first core-host service restart through the UI, then record and verify an owner-approved reboot only during a safe maintenance window.
- [ ] Validate node telemetry and a manual two-host movement/rebuild drill once the second VPS is ready.
