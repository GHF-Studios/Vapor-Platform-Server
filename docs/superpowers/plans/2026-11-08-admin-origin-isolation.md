# Admin Origin Isolation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Isolate privileged pages and terminal routing on a configured admin hostname while preserving the public Vapor host.

**Architecture:** Keep Caddy as the single gateway and add an optional `VAPOR_ADMIN_DOMAIN`. When configured, public human entry routes redirect to the admin host, public admin APIs/terminal paths return 404, and the admin host routes Platform and Identity endpoints. Platform mutation guards use the admin origin; Identity's canonical browser origin becomes the admin host. The admin shell always requires a root Identity session; the emergency browser root console remains disabled until the admin host is explicitly configured.

**Tech Stack:** Caddyfile templates, Bash deployment configuration, Rust/Axum origin guards, Python `unittest`, existing shell deployment tests.

**Spec:** `docs/superpowers/specs/2026-11-08-admin-origin-isolation-design.md`

## Global Constraints

- `VAPOR_ADMIN_DOMAIN` is optional until owner DNS setup; absence must not break existing public operation.
- Once configured, public `/admin*`, `/api/admin*`, and `/emergency-terminal*` are not served on the public hostname.
- Identity session cookies remain host-only; never set `Domain=.vapor.ghf-studios.site`.
- Public/login/admin routes stay behind the same Caddy gateway and service ownership boundaries.
- Emergency shell cannot be enabled unless a distinct admin origin is configured.
- The Platform verifies a root Identity session before serving any `/admin` page or asset, regardless of whether the admin hostname is configured.
- Caddy must validate the candidate before replacing the installed file.

## Review Focus

- Hostname injection/template expansion: invalid domain values must fail before Caddyfile replacement.
- Public-to-admin redirects: preserve path/query for login and CLI flows, but never redirect admin APIs or WebSockets.
- Cookie scope: Identity must set a host-only cookie on the admin host.
- Origin checks: Platform deploy/SSH/console mutations must accept only the configured admin origin after migration.
- Monolith/services parity: both route templates must enforce the same public/admin split.

---

### Task 1: Model and validate the admin host configuration

**Files:**
- Modify: `deploy/scripts/lib.sh`
- Modify: `deploy/scripts/bootstrap-ubuntu.sh`
- Modify: `deploy/scripts/deploy.sh`
- Modify: `deploy/scripts/health-check.sh`
- Test: `deploy/tests/test_admin_origin_config.py`

**Interfaces:**
- `VAPOR_ADMIN_DOMAIN` is blank or a single valid DNS host distinct from `VAPOR_DOMAIN`.
- `VAPOR_PLATFORM_ADMIN_ORIGIN` is `https://$VAPOR_ADMIN_DOMAIN` when configured; otherwise it falls back to the existing public origin for migration only.
- `VAPOR_IDENTITY_PUBLIC_ORIGIN` follows the same canonical browser origin selection.

- [x] Add tests for absent, valid, equal-to-public, malformed, and injection-shaped admin domains.
- [x] Implement strict host validation before writing configuration or rendering Caddy.
- [x] Make bootstrap persist the optional admin domain and pass the canonical browser origin to Identity configuration.
- [x] Make deployment use the configured admin origin for Platform browser mutation guards and Identity callbacks.
- [x] Add admin-host route checks only when the admin hostname is configured; retain the public checks.
- [x] Run focused configuration tests and shell syntax checks.

### Task 2: Split Caddy public and admin host routes

**Files:**
- Modify: `deploy/caddy/Caddyfile.template`
- Modify: `deploy/caddy/Caddyfile.services.template`
- Modify: `deploy/caddy/Caddyfile.services.example`
- Modify: `deploy/scripts/install-caddy.sh`
- Test: `deploy/tests/test_admin_origin_config.py`
- Test: `deploy/tests/test_service_deploy_map.py`

**Interfaces:**
- Public host keeps public services/static routes. If an admin host is configured, `/admin*`, `/login*`, `/logout`, and `/cli/ssh/*` redirect to the admin host preserving URI/query; `/api/admin*` and `/emergency-terminal*` return 404.
- Admin host serves `/admin*`, `/api/admin*`, Identity browser/API routes, CLI SSH authorization routes, and the terminal forward-auth/socket proxy. `/` redirects to `/admin`.
- Without `VAPOR_ADMIN_DOMAIN`, existing host routing remains unchanged, except the emergency shell's own explicit disabled gate.

- [x] Add template assertions proving public/admin route separation for both runtime modes.
- [x] Implement bounded template generation for the optional admin-host server block.
- [x] Validate `VAPOR_ADMIN_DOMAIN` before interpolating it and keep candidate Caddy validation before atomic replacement.
- [x] Confirm the internal terminal authorize callback remains unreachable via Caddy's public path on both hosts.
- [x] Run the Caddy/template tests and shell syntax checks.

### Task 3: Bind Platform admin mutations and emergency-console enablement

**Files:**
- Modify: `src/deploy_requests.rs`
- Modify: `src/ssh_access.rs`
- Modify: `src/emergency_console.rs`
- Test: existing Rust module tests and new focused environment/origin tests.

**Interfaces:**
- Platform's admin origin resolver reads `VAPOR_PLATFORM_ADMIN_ORIGIN`, then the existing public-origin setting only for the no-admin-domain migration state.
- Emergency-console enablement additionally requires a configured admin origin that is distinct from the public host.

- [x] Add validation tests for distinct HTTPS admin origins and malformed/same-host cases.
- [x] Fail console enablement closed unless the admin origin is distinct and canonical.
- [x] Implement the shared resolver without changing Identity role semantics or service-owned behavior.
- [x] Run focused Rust tests and formatting checks limited to modified Rust files.

### Task 3b: Gate the admin shell before serving pages and assets

**Files:**
- Modify: `src/admin.rs`
- Test: `src/admin.rs` module tests

**Interfaces:**
- All `/admin` paths, including page fragments and `admin.css`/`admin.js`,
  require a root Identity session before the route handler runs.
- A failed shell authorization returns a standalone, private no-store HTML
  response with no admin navigation, styles, or scripts.
- `/api/admin/*` continues to authorize independently in its owning handlers.

- [x] Add coverage that missing-session requests cannot reach page or asset handlers.
- [x] Verify the response is private/no-store and contains no admin shell content.
- [x] Run the Platform admin test module and `cargo check --locked`.

### Task 4: Docs, integration review, and deployment gate

**Files:**
- Modify: `docs/superpowers/specs/2026-11-08-emergency-console-design.md`
- Modify: `docs/operations-evidence-and-recovery-contract.md`
- Modify: `deploy/README.md`

- [x] Document DNS, Caddy, Identity-origin, host-only-cookie, and admin health verification steps.
- [x] Run all local Python tests, the Rust test suite, and Caddy validation for all four rendered configurations.
- [x] Review Caddy and Origin paths in monolith and services mode.
- [ ] Deploy source with the optional admin host unset; verify public website/dashboard remain available and console remains disabled.
- [x] Do not configure the new hostname or enable the console until DNS/HTTPS/login checks are complete.
