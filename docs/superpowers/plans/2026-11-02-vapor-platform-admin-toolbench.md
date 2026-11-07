# Vapor Platform Admin Toolbench Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the Identity-only `/admin` page with one consistent, safe toolbench whose sections remain owned by Identity, Registry/Workshop, Artifact, the future Build Cache service, and Platform operations.

**Architecture:** Platform Server owns the shared page frame, navigation, static assets, and overview. Each service owns its admin section handlers, data, authorization checks, state changes, and audit entries. The root composes section routes and never reads service databases or implements their business rules.

**Tech Stack:** Rust, Axum 0.8, current server-rendered HTML and forms, small shared CSS/JavaScript assets served by Platform Server, Identity sessions, audience-bound service authorization.

**Spec:** `docs/superpowers/specs/2026-11-02-vapor-platform-admin-toolbench-design.md`.

## Global Constraints

- One `/admin` entry point presents the shell, module navigation, and a landing page.
- Each service owns its section routes, state, APIs, and audit records.
- Identity sessions are the browser identity source; every service checks capability at the action endpoint.
- Every admin data/action route requires Identity authorization for `root`; only the data-free shared shell and static assets may be public.
- Cross-service calls use the established server-side authorization contract; services never inspect another service's database or copy roles.
- All state-changing admin actions record actor, capability, action, target, timestamp, outcome, and correlation/operation ID in the owning service's audit stream.
- No arbitrary command execution, shell, SQL, generic filesystem access, or secret viewing through the browser.
- Admin responses are private/no-store; CSRF protections, same-origin rules, safe output encoding, and explicit confirmation apply.
- Preserve independent service rebuild/deploy and one VPS composed runtime.

## Review Focus

- Missing/expired/non-root session — public shell reveals navigation only; every section/data/action route denies and makes no state change.
- Cross-site form submission — CSRF test rejects it before the service handler mutates state.
- Stored markup in Steam names, package labels, release metadata, or audit values — HTML output test confirms escaping and no script execution.
- A service is down while the shell loads — overview marks only that module unavailable and other sections remain usable.
- Admin operation returns an ambiguous or partial result — service test verifies a correlated audit/result record and no false success display.

---

### Task 1: Implement Platform-owned shell, common assets, and module navigation

**Files:**
- Create: `src/admin.rs`
- Create: `src/admin_assets.rs` or `resources/admin/` for shared CSS/JS
- Modify: `src/main.rs`
- Modify: `Cargo.toml` only if a dependency already in the workspace is required
- Test: `src/admin.rs` route/render tests

**Interfaces:**
- `GET /admin` and `/admin/` render the shared, data-free landing page and navigation; the page itself may be viewed before login.
- `GET /admin/assets/admin.css` and `/admin/assets/admin.js` serve versioned, static shared assets.
- Initial module IDs and paths: `platform` → `/admin`, `operations` → `/admin/operations`, `workshop` → `/admin/workshop`, `identity` → `/admin/identity`, `artifacts` → `/admin/artifacts`, `build-cache` → `/admin/build-cache`.
- Each service returns a complete page with an escaped service-owned `<main id="admin-content">`; shared static assets add the same navigation shell. A module's required capability and availability label are navigation metadata only; they never authorize an action.

- [x] **Step 1: Add shell tests** `admin_root_lists_fixed_modules`, `public_shell_contains_no_operator_data`, `operator_responses_are_private_no_store`, and `status_text_is_html_escaped`.
- [x] **Step 2: Confirm the shell behavior with focused Platform tests.**
- [x] **Step 3: Implement the minimal Platform shell and responsive shared styles** using existing web technology; no frontend framework is introduced. The shared script wraps only the server-rendered main node and creates navigation from fixed descriptors; it never injects database values as HTML.
- [x] **Step 4: Add a module-unavailable state** so a missing future service remains visible as planned/unavailable rather than disappearing.
- [x] **Step 5: Run the shell tests and Platform formatting/Clippy/Rustdoc**; confirm no server-secret values are rendered.

### Task 2: Move Identity admin into its service-owned section

**Files:**
- Modify: `Vapor-Identity-Server/src/lib.rs`
- Modify: `Vapor-Identity-Server/src/browser_handlers.rs`
- Modify: `Vapor-Identity-Server/src/util.rs`
- Test: Identity browser-handler tests
- Modify: `src/main.rs` to mount Identity section routes below `/admin/identity`

**Interfaces:**
- `GET /admin/identity` renders the existing profile/role/audit dashboard content within the shared shell contract.
- `POST /admin/identity/roles/grant` and `/admin/identity/roles/revoke` retain Identity-owned authorization and audit.
- `/login`, `/logout`, Identity APIs, and existing emergency CLI behavior remain Identity-owned. The former standalone Identity `/admin` route and old form paths are removed; there is no redirect or compatibility alias.

- [x] **Step 1: Add Identity tests** for root access, escaped profile fields, CSRF, last-root protection, and actor/subject audit records.
- [x] **Step 2: Run focused Identity tests** covering protected browser-handler behavior.
- [x] **Step 3: Remove the standalone Identity `/admin` route and old form paths; expose Identity administration only under `/admin/identity`** in `HostRouters`, with no redirect or compatibility alias.
- [x] **Step 4: Render Identity content using shared Platform styles and navigation** while keeping all role decisions and writes inside Identity.
- [x] **Step 5: Run Identity and Platform validation**; confirm the root `/admin` shell no longer collides with Identity routes.

### Task 3: Add Platform authorization client, overview, and health summaries

**Files:**
- Create: `src/admin_auth.rs`
- Modify: `src/admin.rs`
- Modify: `src/main.rs`
- Modify: `Cargo.toml` for HTTP/JSON dependencies required by the authorization protocol
- Modify: Platform environment/deployment templates for the loopback Identity URL and service-only token variable
- Modify: `Vapor-Identity-Server/src/authorization_handlers.rs` and `Vapor-Identity-Server/src/lib.rs` to allow the dedicated `vapor-platform-admin` audience
- Modify: service public readiness/status endpoints only where they lack a secret-free readiness summary
- Test: Platform authorization/overview tests and Identity audience tests

**Interfaces:**
- `admin_auth::authorize_root_with_client(headers, token, endpoint, client)` extracts the Identity session cookie and calls the loopback Identity authorization endpoint for audience `vapor-platform-admin`; missing config or Identity failure denies the request.
- `GET /api/admin/overview` returns named service health/readiness summaries, current deployed revision/status, and operation links; no secrets, environment values, or internal DB rows.
- Health and feature readiness are separate states; an unavailable service is represented without failing the whole page.
- Privileged deployment/recovery operations are not introduced in this task.

- [x] **Step 1: Add authorization tests** for missing/malformed cookies, missing token, Identity denial/timeout, and a root allow decision.
- [x] **Step 2: Add overview tests** for root authorization, partial service outage, bounded probes, private caching, and probe-body redaction.
- [x] **Step 3: Implement audience-bound Platform authorization** and add a dedicated `vapor-platform-admin` Identity audience/token.
- [x] **Step 4: Implement bounded status aggregation** through explicit service APIs only, with per-service timeout and no database reads.
- [x] **Step 5: Render the overview using shared status components**; show partial service state when a health source is unavailable.
- [x] **Step 6: Run Platform and Identity focused validation**; one failed probe does not suppress the other service results and authorization failures deny.

### Task 4: Add Registry-owned Workshop/catalog admin section

**Files:**
- Modify: `Vapor-Registry-Server/src/lib.rs`
- Create: `Vapor-Registry-Server/src/admin.rs`
- Modify: `Vapor-Registry-Server/src/db.rs`
- Modify: `Vapor-Registry-Server/src/types.rs`
- Modify: `src/main.rs` route composition
- Test: Registry admin-handler tests

**Interfaces:**
- `GET /admin/workshop` exposes catalog entries, canonical product/source type, Workshop binding, visibility, and version/channel state to authorized operators.
- Registry mutations remain Registry-owned and have Registry audit records; its composed host interface exposes separate `api` and `admin` routers so Platform can mount them at `/api/registry` and `/admin/workshop`.
- Workshop publish/update actions are omitted or disabled until the Publishing authority/service contract exists; this page never holds a Steam publisher key.

- [x] **Step 1: Add failing tests** for root-only access, catalog listing, HTML escaping, and the absence of mutation controls.
- [x] **Step 2: Run focused Registry tests** and confirm the new handler is missing before implementing it.
- [x] **Step 3: Add the Registry admin section and bounded read API**; expose a maximum of 200 rows and use shared shell assets.
- [x] **Step 4: Keep catalog mutations and Workshop publish controls absent**; unknown Workshop/version binding data is explicitly labelled as untracked.
- [x] **Step 5: Run Registry validation and Platform tests**; access checks execute in the service handler.

### Task 5: Add Artifact release administration

**Files:**
- Modify: `Vapor-Artifact-Server/src/lib.rs`
- Create: `Vapor-Artifact-Server/src/admin.rs`
- Modify: `Vapor-Artifact-Server/Cargo.toml` for bounded Identity authorization and JSON metadata
- Modify: Identity auth config/handler for the dedicated `vapor-artifact-admin` audience
- Modify: `deploy/scripts/configure-authorization.sh` and Artifact env generation
- Test: Artifact admin-handler tests
- Modify: `src/main.rs` route composition

**Interfaces:**
- `GET /admin/artifacts` lists immutable release manifests, signature status, channel pointers, target/platform, and staged/ready state; Artifact exposes separate public and admin routers for composition.
- Existing public release/download endpoints remain unchanged.
- Promote/rollback mutation controls appear only when a separately authorized Artifact lifecycle operation exists.

- [x] **Step 1: Add failing tests** for Identity root checks, staged metadata inventory, and incomplete bundle reporting.
- [x] **Step 2: Run focused Artifact tests** and confirm the inventory functions are absent before implementation.
- [x] **Step 3: Implement read-only release inspection** with bounded scans, path containment, metadata validation, and shared assets.
- [x] **Step 4: Keep channel promotion absent**; inventory is a GET-only router and does not expose signing keys or filesystem paths.
- [x] **Step 5: Run Artifact, Identity, Platform, and Registry validation**.

### Task 6: Add Build Cache section after its service exists

**Files:**
- Modify: `Vapor-Build-Cache-Server/src/lib.rs`
- Modify: `src/main.rs`
- Test: Build Cache admin-handler tests

**Interfaces:**
- `GET /admin/build-cache` shows each target as current, absent, stale, or unavailable; verified stale metadata is visible for diagnosis but never served to clients. It shows the latest successful promotion ID/time. Builder liveness and failed-run history remain unreported until the builder owns a status contract.
- The initial Build Cache section is read-only; only the VPS builder pipeline can replace the current snapshot.

- [x] **Step 1: Add regression/contract tests** for current and stale target state, root authorization, latest promotion outcome metadata, and the lack of browser mutation routes.
- [x] **Step 2: Run focused Build Cache tests**; the stale-lock regression failed against the old behavior before implementation.
- [x] **Step 3: Implement the section against Build Cache-owned snapshot state**; Platform does not read snapshot storage.
- [x] **Step 4: Confirm no browser route can replace the current snapshot or accept arbitrary user archives**; replacement remains builder-pipeline-only.
- [x] **Step 5: Run Build Cache and Platform composition validation**; builder liveness and failure history remain explicitly deferred until an owner-service status contract exists.

### Task 7: Add a read-only Platform operations overview

**Files:**
- Modify: `src/admin.rs`
- Modify: `src/main.rs`
- Modify: service secret-free status summaries only where missing
- Test: Platform operation overview handler tests

**Interfaces:**
- `GET /admin/operations` shows live service status, the latest deploy/restore record, and links to the existing operator runbook.
- `GET /api/admin/operations` authorizes the root session and returns only a 4 KiB-bounded, strict-schema status record; no record is represented explicitly.
- Deploy and restore scripts atomically replace one safe record under `${VAPOR_STATE_ROOT}/operations/latest.json`; this is a latest-status pointer, not an append-only audit stream.
- No deploy/restart/restore mutation endpoint exists; operations remain CLI-owned.

- [x] **Step 1: Add tests** for root checks, bounded strict-schema records, absent/invalid records, redacted fields, and no mutation routes.
- [x] **Step 2: Add the Platform Operations page** and root-authorized latest-record endpoint.
- [x] **Step 3: Implement bounded read-only status reporting** with per-service health timeouts and an atomically written deployment/recovery status pointer.
- [x] **Step 4: Render live service/recovery state and operator-runbook links** using the shared shell; preserve explicit no-record/invalid-record states.
- [x] **Step 5: Run Platform tests and security review**; verify no deployment, command, filesystem, or restore operation can be triggered through HTTP.

### Task 8: Reconcile the server architecture and route documentation

**Files:**
- Modify: `docs/server-domain-boundaries.md`
- Modify: `docs/vertical-slice-contracts.md`
- Modify: `README.md`
- Test: documentation route/boundary consistency scan

**Interfaces:**
- `/admin` is the shared Platform shell. The former standalone Identity dashboard route is removed with no legacy alias; Identity, Registry/Workshop, Artifact, Build Cache, and Platform operations own the sections/routes defined by the approved design.
- The documents retain the rule that the root composes services but does not own their business logic or state.

- [x] **Step 1: Add a route/ownership checklist** for implemented admin routes and retain explicit planned status for Build Cache.
- [x] **Step 2: Update the route map and boundary table** to identify the shared shell, Platform Operations, and service-owned sections.
- [x] **Step 3: Update README topology/route summaries** without claiming the Build Cache section exists.
- [x] **Step 4: Search the server docs for contradictions** that prohibit the shared admin shell or assign `/admin` to Identity; update the contradictory boundary text.
- [x] **Step 5: Review `git diff --check` and route references**; no service state ownership was moved into the root.

## Workstream sequencing

The Vapor Client `workshop-content-superpass` workstream remains the
predecessor for its app-root, installer, Steam-entrypoint, and Workshop-owned
paths. Server work in this repository can proceed independently. Once the
Client workstream reaches a coherent checkpoint, its next queued successor is
the standalone Vapor home and Steam handoff migration; target-snapshot client
installation belongs to that successor and must not be patched in parallel.

## Exit criteria

- One `/admin` shell links to service-owned sections with one consistent layout.
- Identity, Registry, Artifact, and platform data remain owned by their services.
- Every protected route checks authorization server-side and every mutation is audited.
- A single service outage degrades only its own panel.
- No browser-accessible arbitrary command execution or secret display exists.

---

## Productization megapass: turn the first shell into the operator toolbench

The initial shell and section links are a deployed proof of composition and
authorization. They do not satisfy the approved operator-toolbench goal. Use
the productization requirements in the design document above as the source of
truth for this pass.

### Phase A: establish the page contract and fix the shell

- Replace JavaScript-only wrapping of service `<main>` elements with a
  server-rendered shell contract. Keep section body and business logic in its
  owning service.
- Centralize fixed navigation descriptors and route metadata so the shell does
  not maintain separate divergent Rust and JavaScript module lists.
- Group navigation by operator responsibility and distinguish implemented,
  unavailable, and planned features.
- Add shared HTML/CSS components for page headings, badges, filters, tables,
  details, confirmations, async operation status, and empty/error/loading states.
- Review keyboard navigation, accessible names, focus order, narrow viewport
  layout, safe output encoding, and private/no-store behavior.

#### Phase A implementation checkpoint (2026-11-07)

- [x] Define the v1 owner-rendered HTML fragment media type and document its
  bounded shared-shell behavior and independent-service rollout order.
- [x] Migrate Build Cache, Artifact, Registry, and Identity pages, including
  their authentication/error views, to the fragment contract. Platform-owned
  Overview and Operations pages use the same contract.
- [x] Remove whole-document parsing and rewriting from the Platform shell;
  uncontracted HTML now passes through untouched.
- [x] Keep the navigation inventory and page titles derived from the single
  Platform module descriptor, and derive deploy-card target choices from the
  server-side deployment allowlist instead of a JavaScript copy.
- [ ] Complete the remaining hands-on narrow-viewport and assistive-technology review.

  Static review found wide owner-service tables that lacked the shared
  horizontal-scroll treatment. They now use named, keyboard-focusable scroll
  regions. See `docs/deployment-status.md` (Responsive admin tables) for the
  service commits and local test results; real viewport and assistive-tech
  verification is still outstanding.

### Phase B: make Overview and Operations useful

- Overview: service health and feature readiness, active deployment/build
  status, pending approvals, recent completed/failed operations, and clear
  next-action links.
- Operations: separate Deployments, Hosting, Temporary Access, and History
  views with searchable/filterable bounded records.
- Deploy/SSH request cards show immutable request details, capability/target,
  expiry, consequence, status, and outcome. Keep approval decisions in the
  owning authorization/operation handlers.
- Replace opaque failures with bounded operator-facing explanations and a
  correlation ID; never return arbitrary command output or secrets.

#### Focused Phase B implementation checkpoint (2026-11-06)

- [x] Add a read-only Overview queue for deployment, temporary-access, and
  Identity permission requests, with links to each service-owned workflow.
- [x] Add searchable/filterable Operations History across Platform deployment,
  Platform temporary-access, and Identity permission-request owner APIs.
- [x] Preserve owner boundaries and report per-source partial failures; lazy
  load SSH request transition events from the Platform event endpoint.
- [x] Keep request decision and deployment dispatch states independently
  filterable and visible; distinguish the latest deploy/restore pointer from
  request history.
- [x] Let Identity persist expired permission-request transitions and audit
  events; the dashboard displays the owner service's persisted status rather
  than deriving a competing state from timestamps.
- [x] Add explicit refresh and per-source refresh status.
- [x] Show request provenance in owner summaries and review cards without
  exposing or joining owner databases from the Platform shell.
- [x] When approval is persisted but dispatch fails, report both stages and
  any retry eligibility instead of displaying an ambiguous generic 503.
- [x] Add owner-defined keyset pagination to Platform deployment and temporary
  access APIs, and let History load older Platform-owned records on demand.
- [x] Add durable multi-entry deploy/restore operation history, owned by
  Platform. Do not create a cross-service request ledger.
- [x] Identity starts a background expiry sweep with router construction,
  processes up to 10 batches of 100 per pass, and waits 60 seconds between
  passes. Each transition increments the version and writes its expiry audit
  event atomically; large backlogs continue on later passes and already-decided
  requests are untouched.

### Phase C: finish service-owned workbenches

- Identity: profiles, linked providers, roles, permission requests, sessions,
  audit, preview/confirm mutations.
- Registry/Workshop: catalog search, identity/source details, Workshop binding,
  visibility, and explicit publish readiness. No Steam credentials or invented
  publishing control.
- Artifact: release inventory, manifests, signatures, targets, channels and
  validation detail; mutations only where Artifact already owns an audited
  operation.
- Build Cache: per-target current/stale snapshot state, lock freshness, and
  latest successful promotion metadata. Native-builder liveness and failed-run
  history remain deferred until the builder owns a status contract.
- Each section keeps independent build/deploy ownership and renders inside the
  shared shell contract.

### Phase D: whole-journey review and rollout

- Review a logged-out → root-login → overview → inspect service → request/approve
  operation → inspect result journey, including service outage and stale state.
- Independently review service ownership, authz, CSRF, output handling, cache
  headers, accessibility, and failure behavior across the complete toolbench.
- Validate the composed local server, deploy the approved revision through the
  human-approved path, then inspect the actual public dashboard before calling
  this pass complete.

The deployment/cutover incident is a separate operational blocker and must not
be treated as completion of this productization pass.
