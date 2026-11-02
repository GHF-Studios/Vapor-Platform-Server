# Identity Approval Requests Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let root operators issue revocable request-only API credentials and approve role changes through Identity-owned, audited tickets.

**Architecture:** Identity stores scoped credential hashes and permission requests in its existing SQLite database. Human mutations remain Identity-session/root authorized; request credentials can only create/list their own requests. Existing role invariants are revalidated transactionally at approval.

**Tech Stack:** Rust, Axum, SQLite/SQLx, existing Identity session and audit code.

**Spec:** `docs/superpowers/specs/2026-11-02-human-approved-operations-design.md`

## Global Constraints

- Identity owns permission policy, roles, database state, and audit records.
- Do not commit secrets or raw API credentials; persist only one-way hashes.
- Do not permit request credentials to approve, reject, or directly mutate roles.
- Normal dashboard role changes require an explicit approval transition.
- Existing first-root and server-local break-glass safeguards remain explicit.
- Export/import continues to include all Identity state and migrations.
- Commit and push Identity-owned changes to its review branch, then update the
  Platform root gitlink; do not advance `main` or deploy.

## Review Focus

- Expired/revoked request token: reject before creating or listing anything; test token expiry and revocation.
- Concurrent duplicate approval: apply at most once; test competing expected-version transitions.
- Subject roles change while pending: mark stale and require a new request; test state revalidation.
- Last active root removal: reject on approval; test against the existing invariant.
- Root session form mutation from a foreign Origin: reject without changing ticket state; test same-origin enforcement.

---

### Task 1: Request-only automation credentials

**Files:**
- Modify: `Vapor-Identity-Server/src/db.rs`
- Modify: `Vapor-Identity-Server/src/types.rs`
- Modify: `Vapor-Identity-Server/src/main.rs`
- Create: `Vapor-Identity-Server/src/automation_credentials.rs`
- Modify: `Vapor-Identity-Server/src/lib.rs`
- Modify: `Vapor-Identity-Server/README.md`
- Test: `Vapor-Identity-Server/src/automation_credentials.rs`

**Interfaces:**
- `POST /v1/admin/automation-credentials` creates a label, finite expiry, and fixed allowlisted scopes; response includes the bearer secret once.
- `GET /v1/admin/automation-credentials` lists metadata only; `POST /v1/admin/automation-credentials/{id}/revoke` revokes.
- `POST /v1/internal/automation/verify` accepts the caller's bearer and service `audience`/`scope`; it returns principal id, credential id, and only verified scope on success.
- Use scopes `platform:deploy-request:create` and `identity:role-request:create`; reject unknown scopes.
- Store SHA-256 token digest, not raw secret. Verify requests require the configured internal Platform service token, loopback-only routing, strict DTOs, and body limits.

- [ ] **Step 1: Write failing credential persistence and scope tests**
  - Test secret is returned only from create and is not present in database/export.
  - Test unknown scope, expired token, revoked token, wrong service audience, and wrong internal token are rejected.
  - Test valid token gets only the matching requested scope.
- [ ] **Step 2: Run the focused tests and confirm failure**
  - Run: `cargo test --locked --manifest-path Vapor-Identity-Server/Cargo.toml automation_credentials`
- [ ] **Step 3: Implement credential migration and handlers**
  - Add versioned table/index migration, root-session issuance/revocation/listing, secret hashing, expiry bounds, internal verification handler, and route wiring.
- [ ] **Step 4: Run the focused tests and confirm pass**
  - Run: `cargo test --locked --manifest-path Vapor-Identity-Server/Cargo.toml automation_credentials`
- [ ] **Step 5: Document the one-time-secret and request-only contract**
  - Update Identity API/operations docs with expiry, scopes, revocation, and service-token environment names; use placeholders only.

### Task 2: Permission request lifecycle and transactional apply

**Files:**
- Create: `Vapor-Identity-Server/src/permission_requests.rs`
- Modify: `Vapor-Identity-Server/src/db.rs`
- Modify: `Vapor-Identity-Server/src/types.rs`
- Modify: `Vapor-Identity-Server/src/main.rs`
- Modify: `Vapor-Identity-Server/src/admin_handlers.rs`
- Modify: `Vapor-Identity-Server/src/persistence.rs`
- Test: `Vapor-Identity-Server/src/permission_requests.rs`

**Interfaces:**
- `POST /v1/admin/permission-requests` accepts exact `grant|revoke`, allowlisted role, and verified SteamID64/GitHub login; accepts root session or `identity:role-request:create` credential.
- `GET /v1/admin/permission-requests` lists all for root sessions; request credentials can list only their own.
- `POST /v1/admin/permission-requests/{id}/approve` and `/reject` require a root session (not admin bootstrap token or request credential) and expected request version.
- Root-session calls to existing `/v1/admin/roles/grant|revoke` become request creation. Server-local bootstrap/admin-token behavior remains the documented break-glass path.
- Approval atomically re-resolves linked external identities, rechecks role invariants, mutates role, and writes requester/approver audit events.

- [ ] **Step 1: Write failing transition and invariants tests**
  - Test pending request creation, list scoping, request-token denial for approve/reject, root approval, rejection, stale subject state, duplicate approval, expiry, last-root guard, and audit attribution.
- [ ] **Step 2: Run the focused tests and confirm failure**
  - Run: `cargo test --locked --manifest-path Vapor-Identity-Server/Cargo.toml permission_requests`
- [ ] **Step 3: Implement SQLite request storage and transition functions**
  - Use transactions and version/state predicates; add finite expiry, immutable request-body digest, bounded text, and redacted response DTOs.
- [ ] **Step 4: Implement API handlers using existing role validation**
  - Reuse existing grant/revoke checks but call them only inside successful approval; prevent ordinary root API calls from bypassing request creation.
- [ ] **Step 5: Run focused tests and confirm pass**
  - Run: `cargo test --locked --manifest-path Vapor-Identity-Server/Cargo.toml permission_requests`

### Task 3: Identity dashboard review surface and state export

**Files:**
- Modify: `Vapor-Identity-Server/src/browser_handlers.rs`
- Modify: `Vapor-Identity-Server/src/lib.rs`
- Modify: `Vapor-Identity-Server/src/db.rs`
- Modify: `Vapor-Identity-Server/README.md`
- Test: Identity browser/API handler tests and export tests.

**Interfaces:**
- Existing `/admin/identity` displays credential management metadata and permission requests; raw credentials appear only in the immediate creation response.
- Dashboard grant/revoke forms create pending requests. Per-request approve/reject actions call the same APIs with a required Origin check and explicit confirmation text.
- Export/import includes credential digests/revocation and request/audit state; raw secrets are never exportable.

- [ ] **Step 1: Add failing dashboard and export contract tests**
  - Assert forms no longer invoke immediate mutation for root sessions, approval requires Origin, raw secrets are not rendered/listed/exported, and queue outcomes are visible.
- [ ] **Step 2: Run focused tests and confirm failure**
- [ ] **Step 3: Implement the Identity-owned queue/credential UI and export behavior**
- [ ] **Step 4: Run Identity tests and formatting check**
  - Run: `cargo test --locked --manifest-path Vapor-Identity-Server/Cargo.toml`
  - Run: `cargo fmt --manifest-path Vapor-Identity-Server/Cargo.toml -- --check`
- [ ] **Step 5: Commit and push the Identity service change to its review branch**
  - Update the Platform root gitlink only after the service commit is pushed and
    re-run the composed Platform tests against the pinned commit.

**Exit condition:** ordinary role changes require a reviewed pending request, request-only credentials cannot approve, and existing bootstrap constraints remain intact.
