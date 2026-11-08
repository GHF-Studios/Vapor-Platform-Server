# Project Workspaces Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver account-scoped Project lists and membership-gated Project landing pages using Registry-owned metadata and Identity-owned access grants.

**Architecture:** Registry adds first-class Project metadata and asks Identity for membership facts. Identity stores and audits Project grants and exposes only a service-authenticated internal membership API. Homepage renders thin user-facing shells that consume Registry's authenticated user APIs; Platform remains the operator gateway.

**Tech Stack:** Rust, Axum, SQLx/SQLite, HTML/CSS/JavaScript in the existing Homepage service, Python/Rust tests.

**Spec:** `docs/superpowers/specs/2026-11-08-project-workspaces-design.md`

## Global Constraints

- Registry owns Project metadata; Identity owns Project membership and authorization.
- Existing ecosystem/source topology retains its current identity and behavior.
- Project pages require active membership on every private data API request.
- Internal Identity APIs require the dedicated Registry service token and a live session.
- Root and global developer roles do not imply Project membership.
- Project and membership responses use `private, no-store`; unavailable Identity denies access.
- User pages do not render Platform operator navigation or operator assets.

## Review Focus

- Revoked/expired session: Registry returns no Project records; test stale and missing session.
- Revoked/wrong-Project grant: workspace returns generic not-found; test cross-Project denial.
- Identity unavailable or token mismatch: Registry returns service-unavailable, never an empty success; test outage and wrong audience token.
- Malformed Project IDs/roles and HTML metadata: reject invalid writes and escape/render as text; test bounds and XSS payload.
- Archived Project with active membership remains visible as read-only metadata; unknown Project IDs and missing membership remain hidden.
- Empty membership list: dashboard gives a useful empty state distinct from auth/API failure; test each state.

---

### Task 1: Identity public and operator session split

**Files:**
- Modify: `Vapor-Identity-Server/src/db.rs`
- Modify: `Vapor-Identity-Server/src/session_handlers.rs`
- Modify: `Vapor-Identity-Server/src/browser_handlers.rs`
- Modify: `Vapor-Identity-Server/src/lib.rs`
- Test: Identity login/session tests

**Interfaces:**
- Public `/login` and Steam callback create a normal account session in the host-only `vapor_user_session` cookie.
- Operator `/operator/login` and callback preserve the existing root-only `vapor_identity_session` behavior.
- Identity session purpose is explicit; public-user sessions cannot satisfy root-session checks.
- Public and operator origins/callbacks are configured separately when an admin hostname is set; no parent-domain cookies.

- [x] Add tests proving user and operator login purposes/cookies are separate and a user session cannot authorize the root shell.
- [x] Implement normal Steam-authenticated user session creation without granting global roles.
- [x] Preserve the existing root authorization requirements on the operator flow and move its entry/callback to `/operator/login` routes.
- [x] Update Identity callback/origin configuration and run Identity tests plus `cargo check --locked`.

### Task 2: Identity Project membership authority

**Files:**
- Modify: `Vapor-Identity-Server/src/db.rs`
- Modify: `Vapor-Identity-Server/src/lib.rs`
- Create: `Vapor-Identity-Server/src/project_memberships.rs`
- Modify: `Vapor-Identity-Server/src/authorization_handlers.rs`
- Modify: `Vapor-Identity-Server/src/browser_handlers.rs`
- Modify: Identity metadata-only export counts and its tests
- Test: Identity module tests

**Interfaces:**
- Internal `POST /v1/internal/projects/list`: `{ audience, session_token }` → `{ memberships: [{ project_id, role }], expires_at_unix }`.
- Internal `POST /v1/internal/projects/check`: `{ audience, session_token, project_id, minimum_role }` → `{ allowed, role?, expires_at_unix }`.
- Root-only admin grant/revoke endpoints accept stable `project_id`, target `profile_id`, role, expected version, and confirmation; all changes audit actor and subject.
- Roles are exactly `owner`, `developer`, `viewer`; hierarchy is viewer < developer < owner.

- [x] Write migration tests for membership grant, revoke, idempotent regrant, role validation, and audit events.
- [x] Implement membership storage and migration without changing global roles.
- [x] Add server-token/audience checks and live-session checks to internal list/check routes.
- [x] Add root-session/same-origin/confirmation guards to grant/revoke operations and expose a compact Identity-owned management form.
- [x] Include membership and audit row counts in Identity's existing metadata export; rely on the composed SQLite state backup/restore for row preservation rather than inventing a second row-import format.
- [x] Run Identity tests and `cargo check --locked`.

### Task 3: Registry Project directory and access-gated APIs

**Files:**
- Modify: `Vapor-Registry-Server/src/db.rs`
- Modify: `Vapor-Registry-Server/src/types.rs`
- Modify: `Vapor-Registry-Server/src/lib.rs`
- Modify: `Vapor-Registry-Server/src/authorization.rs`
- Modify: Registry tests and README

**Interfaces:**
- Registry schema adds Project rows with stable ID (`namespace/slug`), namespace, slug, display name, summary, and `active|archived` state. Seed `ghf-studios/loo-cast` and `ghf-studios/mad-max-modding` idempotently. Archived Projects remain member-readable and read-only; there are no Project mutation APIs in this slice. Identity root grants use opaque IDs and do not query Registry.
- Public-to-session `GET /v1/me/projects` uses the session cookie and Identity list API; returns matching Registry Projects (active or archived) and role.
- `GET /v1/projects/{namespace}/{slug}` requires the caller's membership at `viewer` or above.
- Missing project/membership returns generic 404; unavailable Identity returns 503; all responses are private/no-store.

- [x] Write migration, seed, membership filtering, failure, and cross-Project tests.
- [x] Add typed Identity client calls for membership list/check using only the Registry audience token.
- [x] Implement Project response/API routes without altering ecosystem routes or semantics.
- [x] Document Project APIs separately from source-topology APIs.
- [x] Run Registry tests and `cargo check --locked`.

### Task 4: Public/operator route separation and account dashboard

**Files:**
- Modify: `Vapor-Platform-Server/deploy/caddy/Caddyfile.template`
- Modify: `Vapor-Platform-Server/deploy/caddy/Caddyfile.services.template`
- Modify: Caddy route assertions and public/admin login tests
- Modify: `Vapor-Homepage-Server/src/lib.rs`
- Create: `Vapor-Homepage-Server/src/dashboard.rs`
- Modify: Homepage docs/tests

**Interfaces:**
- Public `/login` remains on the public host; `/operator/login` is routed to the root Identity login.
- `GET /dashboard` serves a user-only shell that fetches `/api/registry/v1/me/projects` using `vapor_user_session`.
- `GET /projects/{namespace}/{slug}` serves a generic workspace shell that fetches the Registry membership-gated Project route.
- Loading, signed-out, empty, unavailable, and loaded states have distinct copy; metadata uses `textContent` or safe escaping.

- [x] Add route/render tests proving user pages contain no admin shell, navigation, or scripts.
- [x] Route user login and Project pages on the public origin while preserving root-only operator login on the optional admin origin.
- [x] Implement account Project list and Project landing shells with the existing visual system.
- [x] Add links to the public navigation only where appropriate; keep `/admin` operator-only.
- [x] Verify path/query bounds and safe URL construction for namespace/slug.
- [x] Run Homepage tests and `cargo check --locked`.

### Task 5: Cross-service integration and release readiness

**Files:**
- Modify: root `docs/server-domain-boundaries.md`, `docs/decisions-and-backlog.md`, and Project workspace charter
- Modify: root submodule pins after child repositories are committed
- Test: root Rust and deployment/template suites; each changed service CI

- [x] Review Identity and Registry APIs together for token, cookie, error, and response-shape agreement.
- [x] Verify export/import composition includes all new Identity and Registry tables.
- [x] Run each affected service's full test suite, root `cargo check --locked`, and relevant root CI tests.
- [x] Update boundary docs to reflect implemented Project APIs, membership model, and the remaining catalog/workbench gap.
- [x] Commit/push each service-owned implementation and update/push the root submodule pins.
- [ ] Finish composed GitHub CI, then submit through the approved deployment flow.
