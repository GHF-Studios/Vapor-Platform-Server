# Platform Deploy Approvals Implementation Plan

> **Status: Superseded by the integrated 2026-11-03 VPS operations plan.**
> This historical plan describes the five-minute approved-request poller. The
> current design dispatches one systemd unit per approved request and includes
> the temporary SSH grant lifecycle. Use
> [`2026-11-03-vps-operations-and-temporary-ssh.md`](2026-11-03-vps-operations-and-temporary-ssh.md)
> for current work.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace implicit deploy-on-main behavior with an auditable exact-revision request that only runs after a root operator approves it.

**Architecture:** Platform owns a SQLite deploy-request queue and approval API. CI posts a request after passing CI, using an Identity-issued request-only credential; the root dashboard can approve/reject. A root-owned worker atomically claims only approved requests and calls the fixed deployment script with the approved commit and pinned submodule revisions.

**Tech Stack:** Rust/Axum, SQLite/SQLx, Tokio, GitHub Actions, systemd, existing Bash deployment scripts.

**Spec:** `docs/superpowers/specs/2026-11-02-human-approved-operations-design.md`

**Dependency:** Execute after `2026-11-02-identity-approval-requests.md` has established the Identity request-token verification contract.

## Global Constraints

- Platform owns deployment orchestration and its request records; Identity owns credentials, roles, and permission decisions.
- No normal deploy occurs without a root-session approval bound to an immutable commit/tree/gitlink set.
- Request tokens can create and inspect only their own requests; they cannot approve or execute.
- No generic shell, path, Git ref, environment, or command data is accepted from callers.
- Pushes to `main` run CI and create a pending ticket only; the VPS timer may execute only approved tickets.
- The initial bootstrap/migration deploy is a documented operator action; implementation must not deploy to production.
- Preserve per-service rebuilds and full-system export/import behavior.
- Push only to the existing review branch; never merge to `main` or invoke a
  live deploy.

## Review Focus

- Missing or malformed identity token: return unauthorized/unavailable and never queue a request; test API behavior.
- Commit moves or branch head advances after request: deploy only the approved commit if still reachable; test worker validation.
- Duplicate workers/requests: one request has at most one active claim; test atomic claim/version transitions.
- Request asks for a non-deployment action or caller-supplied path: reject strict DTO; test unknown fields and values.
- No approval / expired / rejected ticket: worker does not execute any script; test worker no-op path.

---

### Task 1: Platform deploy-request store and API

**Files:**
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Create: `src/lib.rs`
- Create: `src/deploy_requests.rs`
- Modify: `src/main.rs`
- Modify: `src/admin.rs`
- Modify: `src/admin_auth.rs`
- Modify: `deploy/scripts/deploy.sh`
- Modify: `deploy/scripts/deploy-atlas.sh`
- Modify: `deploy/scripts/lib.sh`
- Test: `src/deploy_requests.rs` and `src/admin.rs`

**Interfaces:**
- `POST /api/admin/deploy-requests` creates a ticket from strict DTO `{revision, tree, submodules, ci_run_url}`; accepts root session with Origin validation or an Identity token with `platform:deploy-request:create`.
- `GET /api/admin/deploy-requests` lists requests for root; request credentials can inspect only their own.
- `POST /api/admin/deploy-requests/{id}/approve` and `/reject` require root session, Origin validation, expected version, and explicit decision; body cannot alter candidate fields.
- `POST /v1/internal/deploy-requests/claim` and `/finish` are loopback-only and use a unique Platform worker secret available only to the worker; claim returns one fixed allowlisted request.
- Request state is Platform-owned SQLite at `/var/lib/vapor-server/platform/platform.sqlite3`; each transition is durable, versioned, and linked to the original request hash.
- Deployment scripts accept an exact validated revision only from the worker; they verify commit reachability from configured branch and expected root tree/submodule gitlinks before build/restart.

- [ ] **Step 1: Write failing queue-state tests**
  - Cover create/list, owner-only listing, invalid DTOs, expiry, reject, approve, duplicate approval, stale version, and immutable candidate digest.
- [ ] **Step 2: Run focused tests and confirm failure**
  - Run: `cargo test --locked --manifest-path Cargo.toml deploy_requests`
- [ ] **Step 3: Add SQLite dependency, shared library entry point, migrations, and repository functions**
  - Use SQL transactions and compare-and-swap status/version checks; retain bounded, redacted DTOs.
- [ ] **Step 4: Add root/request-token auth adapters and Axum request routes**
  - Reuse the existing root-session-to-Identity authorization path; call Identity's internal token verification with a distinct Platform service secret for request tokens.
- [ ] **Step 5: Run focused tests and confirm pass**

### Task 2: Approved exact-revision worker

**Files:**
- Create: `src/bin/vapor-deploy-worker.rs`
- Modify: `Cargo.toml`
- Create: `deploy/systemd/vapor-approved-deploy.service`
- Create: `deploy/systemd/vapor-approved-deploy.timer`
- Modify: `deploy/scripts/install-auto-deploy.sh`
- Modify: `deploy/scripts/deploy.sh`
- Modify: `deploy/scripts/deploy-atlas.sh`
- Modify: `deploy/scripts/export-state.sh`
- Modify: `deploy/scripts/restore-state.sh`
- Test: worker unit tests plus deployment-script contract tests.

**Interfaces:**
- Worker loads only local deployment configuration, claims the next approved request atomically, verifies expiry/state/hash, validates exact Git commit/tree/submodule set, and executes one fixed deploy script.
- Worker has no network-facing command API. Its local credentials are separate from CI/request credentials and stored only in root-readable service configuration.
- Deployment result transitions the request to succeeded/failed and writes the existing operation record; retry requires a new approval ticket unless failure occurred before claim.
- Replace periodic deploy-latest timer with the approved-ticket worker timer. A no-ticket poll exits successfully without modifying the checkout or services.

- [ ] **Step 1: Write failing worker safety tests**
  - Cover no-op with empty queue, no execution for pending/rejected/expired requests, one claim under concurrent workers, unreachable/mismatched revision refusal, and status/result update.
- [ ] **Step 2: Run focused tests and confirm failure**
- [ ] **Step 3: Implement worker binary and exact-revision script contract**
  - Worker constructs fixed executable/arguments from trusted config and ticket fields; script validates again before changing the deployment checkout.
- [ ] **Step 4: Add systemd unit/timer and migration-safe installation**
  - First install disables the old deploy-latest timer and enables only the approved worker. Do not initiate deployment as part of installation.
- [ ] **Step 5: Extend state export/import for Platform SQLite and active tickets**
  - Quiesce Platform/worker during snapshot/restore; expired pending tickets remain non-executable after restore.
- [ ] **Step 6: Run worker, shell syntax, export/restore, and focused Rust tests**

### Task 3: Operations dashboard review UI

**Files:**
- Modify: `src/admin.rs`
- Modify: `src/admin_auth.rs`
- Modify: `docs/vertical-slice-contracts.md`
- Test: admin router/page contract tests.

**Interfaces:**
- `/admin/operations` shows pending, executing, terminal, and expired requests, with exact candidate metadata and explicit approve/reject actions.
- UI actions call the same versioned API used by direct clients and require same-origin confirmation.
- Worker-only routes are excluded from public Caddy routing; request APIs use the public admin prefix and no-store responses.

- [ ] **Step 1: Write failing dashboard/API auth tests**
  - Test unauthenticated access, request credential cannot approve, no mutation on wrong Origin, stale version response, and rendered exact revision/submodule pins.
- [ ] **Step 2: Run focused tests and confirm failure**
- [ ] **Step 3: Implement Operations queue view and safe UI actions**
- [ ] **Step 4: Run Platform tests and formatting check**

### Task 4: CI request-only integration and deployment handoff

**Files:**
- Modify: `.github/workflows/deploy.yml`
- Modify: `.github/workflows/ci.yml`
- Modify: `deploy/scripts/bootstrap-ubuntu.sh`
- Modify: `deploy/scripts/configure-authorization.sh`
- Modify: `deploy/scripts/health-check.sh`
- Modify: `deploy/README.md`
- Create: `docs/human-approved-operations-api.md`
- Modify: `deploy/scripts/test.sh`

**Interfaces:**
- Main push runs the existing CI workflow. Only a successful `workflow_run`
  completion for the protected `main` branch may submit
  `{revision, tree, submodules, ci_run_url}` to Platform using
  `VAPOR_DEPLOY_REQUEST_TOKEN`; it never SSHes to run deploy.
- Missing request token or service URL prevents ticket creation with an actionable CI notice/failure policy documented; it never silently triggers deploy.
- `workflow_dispatch` first runs the same validation for an explicitly selected
  revision reachable from `main`, then creates a request; it cannot approve or
  execute it.
- Deployment/API runbooks document the one-time manual bootstrap rollout, request-token provisioning in GitHub, dashboard review, rejection, expiry, and recovery.

- [ ] **Step 1: Add workflow contract checks**
  - Assert the workflow contains no SSH deploy trigger and includes no approval credential; request token is the only deployment-related secret used by CI.
- [ ] **Step 2: Implement CI metadata capture and request API submission after required checks**
  - Use `workflow_run` gated on successful `CI` completion and `main`, without
    downloading untrusted PR artifacts. Capture commit/tree/gitlinks from that
    run and submit only after required CI succeeds. Manual dispatch performs its
    own validation before submission.
- [ ] **Step 3: Update bootstrap and authorization configuration with placeholders only**
  - Distinct secrets for admin authorization, request-token validation, and worker access; no token values committed.
- [ ] **Step 4: Update docs, Caddy/public-route contract checks, and test recipe**
- [ ] **Step 5: Run full offline Rust suite, shell syntax, Python state tests where available, `git diff --check`, and source secret scan**
- [ ] **Step 6: Commit/push the Platform review branch and Identity submodule pin; report CI/deployment gaps without merging or deploying**

**Exit condition:** CI can create a pending exact-revision request, the VPS cannot deploy it until a root human approves it, the worker cannot execute arbitrary input, and the dashboard exposes both state and audit history. No production deploy is performed by this plan.
