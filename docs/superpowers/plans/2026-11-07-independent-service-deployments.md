# Independent Service Deployments Implementation Plan

> **For agentic workers:** Required sub-skill: `superpowers:executing-plans`.

**Goal:** Add approved, rollback-capable deployment of one pinned service while
preserving the existing full Platform deployment path.

**Architecture:** Platform keeps approval and source validation. Requests carry
an explicit full/service scope and fixed service target. The worker verifies
the same exact root composition for both scopes; the scoped deployer builds,
swaps, restarts, and health-checks only one mapped service, restoring its prior
binary and state on failure. Platform and shared-host changes remain full
deployments.

**Tech Stack:** Existing Rust/Axum/SQLite request flow, fixed root-owned Bash
deployment scripts, systemd service units, Caddy path routing, Python helper
tests, and GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-11-07-independent-service-deployments.md`

## Constraints

- Do not add a new authority system; use existing Identity-backed request and
  approval flow.
- Service targets are fixed to Atlas and the seven service submodules;
  Platform is full scope only.
- The root commit and every submodule gitlink are verified before build.
- Scoped deployment builds from an isolated temporary local clone and leaves
  the canonical deployment checkout and its submodule metadata untouched.
- Scoped deployment is available only in `services` mode and cannot mutate
  shared Caddy, systemd, environment, or topology configuration.
- Preserve services as root-level submodules and keep business logic in each
  service repository.
- Keep operation reports and deployed-source metadata free of secrets.
- Preserve unrelated untracked files and scratch scripts.

## Review Focus

- Malformed scope/target combinations are rejected before queueing or mutation.
- A scoped request cannot choose a command, unit, binary path, or health URL.
- A successful scope never restarts a neighbor or reloads Caddy.
- Any failed build/swap/restart/health check preserves and restores the target's
  prior executable and state; incomplete rollback preserves its backup.
- A prior successful deployed-source manifest stays intact until the new
  target passes health checks and the manifest update commits atomically. The
  manifest lives under the existing exported state tree; schema-v2 categories
  do not change.
- Only one active request per root commit is permitted. A new request with a
  different mode/scope/target conflicts clearly; it must never reuse an
  unrelated active request.

---

### Task 1: Extend the deployment request contract

**Files:**
- Modify: `src/deploy_requests.rs`
- Modify: `src/deploy_request_schema.rs`
- Modify: `deploy/scripts/deploy-request-queue.py`
- Modify: `.github/workflows/deploy.yml`
- Test: existing Rust request tests and `deploy/tests/`

**Interfaces:**
- Request fields: `deployment_scope: "full" | "service"` and
  `service_target: null | fixed service slug`.
- Full requests have no target; service requests have exactly one allowlisted
  target (`atlas`, `homepage`, `docs`, `identity`, `diagnostics`, `registry`,
  `artifact`, or `build-cache`) and require `server_mode="services"`.
- Push to main continues to submit full scope. A main-only `workflow_dispatch`
  accepts a typed target choice and submits a scoped request after the same CI.

- [x] Add request validation tests for full, each service target, invalid
  target, missing target, extraneous target, and monolith/service mismatch.
- [x] Verify the incomplete legacy-selection regression fails before the fix.
- [x] Implement request fields, summary fields, and API validation; preserve the
  full-push request path.
- [x] Update queue claim/verify tests so the worker receives scope and target
  from the immutable request body and rejects mismatches. Test that another
  scope for an active commit conflicts instead of returning that request.
- [x] Add the guarded workflow dispatch; assert it runs only on `main`, requires
  composed CI, and emits only typed scope/target values.
- [x] Run focused request/workflow validation and the full repository test
  recipe.

### Task 2: Implement service-scoped deployment with rollback

**Files:**
- Create: `deploy/scripts/deploy-service.py`
- Create: `deploy/scripts/service_deploy_map.py`
- Modify: `deploy/scripts/run-approved-deploy.sh`
- Modify: `deploy/scripts/deploy-request-queue.py`
- Test: `deploy/tests/test_deploy_service.py` and worker tests

**Interfaces:**
- Worker passes only approved request ID/version, root commit, and a
  server-validated service slug.
- The fixed map defines service package path, binary name, unit name, loopback
  health URL, public gateway check, service state path, and bounded state
  rollback policy.
- Service deployer consumes only the already verified isolated temporary clone;
  it initializes only the selected service submodule and accepts no
  user-provided filesystem path, unit name, executable,
  or URL. The canonical deployment checkout remains untouched by this path.

- [x] Test that each allowlisted target maps to exactly one package, binary or
  static release, unit (where applicable), loopback health endpoint (where
  applicable), state directory, public route, and public status contract.
  Derive each check from actual Caddy routing, including prefix stripping and
  routes that preserve their prefix; do not assume all services expose
  `/healthz` publicly.
- [x] Test that invalid targets, monolith mode, missing/unhealthy units, dirty
  source, and incorrect pins fail before service mutation.
- [x] Test that service scope invokes only the selected package build and unit
  lifecycle; no neighbor unit, Caddy, systemd installer, or env configuration
  command is invoked.
- [x] Test snapshot failure, failure after binary/static-link swap, failure
  after restart, and manifest-write failure; assert the prior binary/release,
  target state, and manifest restore, and incomplete rollback retains its
  backup.
- [x] Preserve `root:<service-group>` ownership on replacement binaries and
  preserve source ownership/modes when restoring service state. Keep
  immutable Artifact/Build Cache payload trees in place during binary-only
  updates to avoid copying large artifacts.
- [x] Implement the fixed map, build staging, per-service quiesce/state backup,
  atomic binary swap, targeted health checks, and bounded rollback reporting.
- [x] Record the deployed-source manifest only after targeted health success
  under `VAPOR_STATE_ROOT/operations`; the existing schema-v2 exporter copies
  this directory, so no archive-category expansion is needed.
- [x] Run focused service-deployer tests, shell syntax checks, and repository
  test recipe.

### Task 3: Preserve composed deployment and recovery semantics

**Files:**
- Modify: `deploy/scripts/deploy.sh`
- Modify: `deploy/scripts/state-bundle-validate.py`
- Modify: `deploy/README.md`
- Modify: `docs/operations-evidence-and-recovery-contract.md`
- Test: recovery validator fixtures

- [x] Confirm full-scope deploy still builds/restarts the complete selected
  topology and writes a complete deployed-source manifest.
- [x] Extend schema-v2 recovery validation/import to validate this optional
  operations manifest when present; old bundles without it remain valid and
  explicitly unknown, never synthesized as current.
- [x] Document why Platform, gateway, Caddy, shared config, and topology changes
  require full scope; service code alone may use scoped deployment.
- [x] Run full repository validation and review the exact candidate deployment
  diff before updating any live host.

## Validation limits

- The complete repository test recipe passed on Windows (69 Python tests;
  nine POSIX-specific cases skipped) and on GitHub Actions Ubuntu 24.04
  (69 Python tests plus the Rust suites for Platform and all service
  submodules). The Linux CI run is
  [42338788185](https://github.com/GHF-Studios/Vapor-Platform-Server/actions/runs/42338788185).
- No live deployment was run. The prior temporary SSH grant expired; installing
  this on the VPS still requires the approved deployment flow and a fresh
  authorization.
- A full VPS export/restore round-trip was not executed. The optional manifest
  is validated in schema-v2 bundles and remains ordinary state-tree data.
