# Independent Platform Server Deployment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deploy Platform Server independently as an approved, rollback-capable service target.

**Architecture:** Add a root-package service entry to the existing fixed target map. The scoped deploy helper will verify the approved root revision, build only `vapor_platform_server`, snapshot Platform state, replace/restart only its unit, health-check, and roll back on failure. Workflow classification maps Platform server Rust sources to this target while keeping the ops helper and ambiguous root changes full-scope.

**Tech Stack:** Rust, Python deploy helpers, GitHub Actions, SQLite request queue, systemd.

**Spec:** `docs/superpowers/specs/2026-11-08-independent-platform-server-deployment-design.md`

## Global Constraints

- Existing operator approval, ordering, audit, and request-scoped worker remain authoritative.
- Root changes outside the explicit Platform server source set continue to select full deployment.
- Only the Platform server binary and its service state may be mutated by the `platform` target.
- No direct SQL writes, live deployment, or approval bypass in implementation verification.
- Preserve untracked scratch files and unrelated worktree state.

## Review Focus

- Root-source targets must still verify the exact root commit/tree and clean detached checkout; they must not fake a gitlink.
- The Platform root binary depends on all root-pinned service crates, so its isolated build checkout must initialize and verify every pinned submodule while deployment mutations remain limited to the Platform target.
- `vapor_platform_ops` edits must not be misclassified as a Platform-server-only deployment.
- Platform database state must be snapshotted after stopping the service and restored on rollback.
- Mixed Platform/submodule commits must remain full composition.
- Unknown root paths must fail toward full deployment, never `none` or an unintended narrow target.

---

### Task 1: Extend the fixed target map and deployment helper

**Files:**
- Modify: `deploy/scripts/service_deploy_map.py`
- Modify: `deploy/scripts/deploy-service.py`
- Modify: `deploy/scripts/run-approved-deploy.sh`
- Test: `deploy/tests/test_service_deploy_map.py`
- Test: `deploy/tests/test_deploy_service.py`

**Interfaces:**
- Target name: `platform`.
- Metadata: root package path `.`, binary `vapor_platform_server`, unit `vapor-platform-server.service`, loopback health `http://127.0.0.1:7110/healthz`, public health `/healthz`, state name `platform`, state rollback `snapshot`.
- Root-service source identity is the approved root commit itself; submodule targets continue to require a matching gitlink.

- [ ] **Step 1: Write failing map tests** asserting the exact `platform` metadata, target lookup, and existing allowlist shape.
- [ ] **Step 2: Run the focused map test** and confirm `platform` is currently rejected/missing.
- [ ] **Step 3: Write failing deploy-helper tests** for exact root commit/tree verification without gitlink lookup and successful deployment touching only Platform unit/binary/state.
- [ ] **Step 4: Run the focused helper tests** and confirm the new root-service behavior is absent.
- [ ] **Step 5: Implement root-service source verification** and `--bin <target binary>` scoped build while preserving current service and Atlas checks.
- [ ] **Step 5a: For the root-service target, initialize and verify all pinned path-dependency crates; continue initializing only the selected gitlink for submodule targets.**
- [ ] **Step 6: Test rollback paths** for binary replacement, manifest update, health-check, and state snapshot failures; assert unrelated units are untouched.
- [ ] **Step 7: Run both focused test modules** and require all map/deploy-helper tests to pass.

### Task 2: Thread the target through approval and execution allowlists

**Files:**
- Modify: `src/deploy_requests.rs`
- Modify: `src/admin.rs`
- Modify: `deploy/scripts/deploy-request-queue.py`
- Modify: `deploy/scripts/deploy-service.py`
- Modify: `deploy/scripts/run-approved-deploy.sh`
- Modify: `.github/workflows/deploy.yml`
- Modify: `deploy/tests/test_deploy_request_queue.py`
- Modify: Rust deploy-request/admin tests in `src/deploy_requests.rs` and `src/admin.rs`

**Interfaces:**
- `platform` remains `deployment_scope="service"`, `server_mode="services"`.
- The shared target list comes from `service_deploy_map.py` where feasible; API and Rust checks must include the exact same allowlisted ID.
- Workflow dispatch exposes `platform` as an explicit target.

- [ ] **Step 1: Add failing queue/API tests** proving `platform` is accepted as a service target while invalid target/scope combinations remain rejected.
- [ ] **Step 2: Run focused Python/Rust tests** and confirm rejection occurs at each currently independent allowlist.
- [ ] **Step 3: Add `platform` to queue, API, admin metadata, and deploy-helper choices**; avoid adding any caller-supplied paths or commands.
- [ ] **Step 4: Re-run focused queue/API/admin tests** and confirm the request target survives creation, claim, verify, and rendering.

### Task 3: Classify changes and preserve full-deploy fallback

**Files:**
- Modify: `deploy/scripts/deployment_scope.py`
- Modify: `.github/workflows/deploy.yml`
- Modify: `deploy/tests/test_deployment_scope.py`
- Modify: `deploy/tests/test_approved_deploy_exec.py`
- Modify: `docs/github-deployment.md`

**Interfaces:**
- Only `src/main.rs`, `src/admin.rs`, `src/admin_auth.rs`, `src/admin_gateway.rs`, `src/deploy_request_schema.rs`, `src/deploy_requests.rs`, `src/emergency_console.rs`, `src/operation_history.rs`, `src/pagination.rs`, `src/platform_ops_client.rs`, `src/recovery.rs`, `src/ssh_access.rs`, `src/ssh_key.rs`, and `src/ssh_schema.rs` select `platform`.
- `src/bin/vapor_platform_ops.rs`, shared helper modules `src/recovery_status.rs` and `src/ssh_policy.rs`, other/unknown source paths, `Cargo.toml`, `Cargo.lock`, deployment config/scripts, mixed Platform+submodule edits, and multiple service edits select `full`.
- Docs/tests-only changes remain `none`; one submodule remains its single target.

- [ ] **Step 1: Add failing classifier tests** for each explicitly Platform-owned file, shared ops-helper modules, ops-helper binary, mixed Platform/service, and unknown-root cases.
- [ ] **Step 2: Run the classifier tests** and record the current incorrect full/none result.
- [ ] **Step 3: Implement only the explicit Platform source mapping**; retain the safe full fallback for all other root inputs.
- [ ] **Step 4: Add workflow target and docs coverage** for workflow-dispatch selection and push classification.
- [ ] **Step 5: Run classifier/workflow/helper test modules and `git diff --check`.**

### Task 4: Full composed verification and publish source

**Files:**
- Review all files from Tasks 1–3; no additional code scope.

- [ ] **Step 1: Run the complete Platform test recipe** across root and all service submodules.
- [ ] **Step 2: Review the diff against the design**, especially target allowlist consistency, root source verification, rollback, and unknown-path fallback.
- [ ] **Step 3: Commit and push the implementation branch.**
- [ ] **Step 4: Wait for Linux and Windows CI to pass.**
- [ ] **Step 5: Fast-forward `main` only after CI passes; verify the workflow creates a `platform`-scoped request and does not mutate the live VPS without approval.**

