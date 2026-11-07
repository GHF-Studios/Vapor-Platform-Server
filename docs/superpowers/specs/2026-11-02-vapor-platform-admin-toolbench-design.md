# Vapor Platform Admin Toolbench

**Status:** Approved by owner on 2026-11-02
**Date:** 2026-11-02

## Intent

Turn Vapor's server administration from a collection of CLI-only workflows and
service-specific pages into one coherent, safe web toolbench. Operators should
be able to inspect and manage the Workshop/catalog, identity, artifact/release,
build-cache, and platform-hosting concerns from a consistent layout without
making the Platform Server the owner of those services' business state.

This design extends the approved standalone Vapor/product-boundary design. It
does not replace that design's service ownership, VPS topology, product
authority, or release/cache decisions.

## Current state

- `Vapor-Platform-Server` composes service routers into the single VPS process
  and owns deployment and whole-system operations.
- `/admin` is currently served by Identity and focuses on Identity profile and
  role operations. The Identity service owns its login/session model.
- Registry has public catalog reads but no administration UI yet.
- Artifact serves signed Vapor application-release downloads; it has no
  administration UI.
- Build-cache orchestration is a planned separate service concern, not part of
  the existing Artifact service.
- Existing platform-boundary notes classify a cross-service admin UI as future
  work. This design supersedes that status for the toolbench direction only;
  service boundaries remain in force.

## Goals

- Provide one authenticated `/admin` entry point with a consistent navigation
  shell and shared visual language.
- Give the operator clearly separated sections for:
  - Overview and platform health/deployment state.
  - Workshop and catalog records, bindings, and publishing status.
  - Identity, linked accounts, roles, sessions/readiness, and audit.
  - Vapor application releases and their manifests/channels.
  - Build-cache target snapshots and builder status when that service exists.
- Keep each section's business rules, state, APIs, and audit records in its
  owning service.
- Replace routine CLI wizard work with discoverable web workflows while
  retaining a restricted emergency/operator CLI path.
- Make administrative actions explicit, auditable, capability-checked, and
  recoverable where the action changes state.

## Non-goals

- A browser terminal, arbitrary shell/SQL, generic filesystem browser, or
  secrets viewer.
- Moving service handlers, databases, or policy into the Platform Server root.
- Letting a dashboard declaration or browser-supplied data grant authority.
- Putting Steam publisher credentials or signing private keys in the browser.
- Treating the Workshop transport as the canonical catalog or identity source.
- Making public users administer services.

## Approaches considered

1. **Shared shell with service-owned sections (selected).** The platform host
   provides common navigation, page framing, and a platform overview. Services
   contribute their own admin pages and actions behind their APIs and
   authorization checks. This gives one toolbench while preserving current
   ownership boundaries.
2. **Central admin application that calls every service.** It can unify the UI
   quickly, but service workflows and policy tend to leak into a central app;
   this duplicates authority and couples the root to every service API.
3. **Independent service dashboards with links between them.** This preserves
   ownership but leaves operators with several disconnected layouts and does
   not meet the requested toolbench experience.

Approach 1 was selected because it combines a unified operator experience with
the existing rule that service repositories own their business logic.

## Ownership and module map

### Platform shell and overview — `Vapor-Platform-Server`

The root platform repository owns the `/admin` shell, shared navigation and
layout contract, and a read-only overview of composed-service health, deployed
revision/status, and operational readiness. It may aggregate service-published
status summaries through explicit service APIs. It must not read service
databases or implement their policy.

Platform actions are limited to named, allowlisted operations backed by the
existing deployment/recovery workflows. There is no arbitrary command
execution endpoint. Actions that restart, deploy, or restore must expose their
impact, require explicit confirmation, and report the resulting operation ID
and status. Secrets and private configuration values are never rendered.

The implemented first slice is intentionally read-only: `/admin/operations`
shows health, runbooks, and a latest deploy/restore status pointer. Deploy and
restore remain CLI-owned until an independently reviewed action contract,
confirmation flow, authorization/audit path, and asynchronous operation status
model exist.

### Workshop and catalog — Registry plus future publishing boundary

Registry owns canonical catalog records, content identity/type, visibility,
Workshop item bindings, and version/channel metadata. Its admin section owns
catalog inspection and Registry-authorized catalog changes.

Workshop upload, update, or moderation workflows belong to the separate
publishing boundary when implemented. The dashboard may present publishing
status alongside Registry data, but does not put Steam credentials in the
Platform host or make Workshop tags authoritative. Steamworks authority and
Identity authorization remain separate checks.

### Identity — `Vapor-Identity-Server`

Identity owns login/session validation, linked external accounts, roles,
authorization state, and its audit log. The old standalone Identity `/admin`
page is removed and replaced by the shared Platform toolbench; there is no
legacy URL or compatibility alias. Profile, role, and audit tools belong in the
new Identity section at `/admin/identity`, with Identity authorization checks
and audit trail remaining service-owned.

### Releases — `Vapor-Artifact-Server`

Artifact owns immutable Vapor release bundles, signatures, manifests, and
channel pointers. Its section may inspect release/channel readiness and expose
the service's explicitly authorized promotion/rollback workflow if/when that
workflow is implemented. Artifact remains separate from content uploads and
target build-cache snapshots.

### Target build snapshots — future build-cache service

The separate build-cache service owns target snapshot manifests, publication
state, builder results, and snapshot retention/replacement operations. Its UI
may be linked from the same toolbench. It must not be folded into Artifact's
Vapor-app release semantics or Platform root state.

## Shared UI and routing contract

- One `/admin` entry point presents the shell, module navigation, and a landing
  page. Each service owns its section routes below a stable module path:
  `/admin/identity`, `/admin/workshop`, and `/admin/artifacts`; Platform owns
  the read-only `/admin/operations` section.
- The shell owns shared typography, spacing, navigation, status badges,
  confirmations, and empty/error/loading states. Service sections use the same
  visual and interaction contract while remaining rendered/implemented by their
  owning service.
- Each module provides a small descriptor: stable ID, display name, route,
  required capability, and availability/readiness summary. The shell uses this
  only for navigation and status; it does not infer permissions from the
  descriptor.
- The platform overview distinguishes service health from feature readiness.
  Missing or not-yet-built modules appear as unavailable/planned rather than
  silently disappearing. Feature readiness is `not reported` until the owning
  service provides a safe status DTO; HTTP health alone never implies that a
  service feature is configured.
- The current standalone Identity `/admin` dashboard route is removed and
  replaced by the shared Platform shell at `/admin`. Identity's supported
  dashboard functionality is exposed only within its service-owned
  `/admin/identity` section; there is no legacy dashboard alias or redirect.
- Modules must continue to work when composed into the single VPS process and
  retain independent service build/deploy boundaries.

## Security and operational safety

- Identity sessions are the browser identity source. Every service checks the
  needed capability at the action endpoint; hiding a button is not
  authorization.
- Cross-service calls use the established server-side authorization contract
  with a distinct service token for each audience. Services never inspect
  another service's database or copy its roles.
- All state-changing admin actions record actor, capability, action, target,
  timestamp, outcome, and correlation/operation ID in the owning service's
  audit stream. The shell can aggregate audit summaries only through explicit
  read APIs.
- State-changing or externally visible actions use a review/confirm step that
  names the target and consequence. Long-running work returns a status handle
  rather than blocking a browser request.
- Responses containing session or operator data are private/no-store. CSRF
  protections, same-origin rules, and safe output encoding are mandatory for
  browser forms and interactive pages.
- Bootstrap tokens and emergency server credentials remain CLI-only and are
  never exposed through the dashboard.
- The dashboard exposes no arbitrary path, SQL, shell, environment, or secret
  access. Logs, if later exposed, are bounded, redacted, and read-only.

## Delivery sequence

1. Define the shared admin-module descriptor, shell route contract, and
   Identity-backed browser authorization behavior.
2. Remove the standalone Identity `/admin` page and expose Identity-owned
   dashboard functionality only from the new `/admin/identity` section.
3. Add the platform Overview using explicit health/deployment summaries.
4. Add Registry/catalog inspection, then publishing actions only after the
   publishing authority contract exists.
5. Add Artifact release inspection and authorized lifecycle actions.
6. Add build-cache administration when its service and VPS builder workflow
   exist.
7. Add a read-only Platform Operations overview for health, latest deploy/restore
   status, and runbooks; keep deploy/recovery actions CLI-owned for this slice.

## Decisions left to implementation planning

- The exact cross-repository mechanism for shared shell assets and module
  descriptors must avoid a shared database or root-owned service logic.
- The initial shell should deliver a common responsive layout using existing
  project web technologies rather than introducing a new frontend stack unless
  repository evidence demonstrates a need.
- Initially expose read-only summaries where mutation workflows or their audit
  contracts do not yet exist.
- Route names are proposed stable paths and should be reconciled with existing
  routing before implementation.

## Superseded boundary note

The “Do not build yet” entry in `docs/server-domain-boundaries.md` that
prohibits a full global admin dashboard is superseded by this accepted
direction. It is not permission to centralize business logic: the shared shell
and service-owned sections described here are the required boundary.

## Productization pass after the first deployed shell

The first deployed shell is a routing and authorization proof, not the finished
operator toolbench. The next pass must make routine server work understandable
and usable without pretending that a list of links and health cards is a
complete admin product.

### Shared frame

- Render the shared frame as part of the server response. Navigation and page
  framing must not depend on client JavaScript finding and wrapping a `<main>`
  element after load.
- Use a stable left navigation grouped by responsibility: Overview;
  Catalog (Workshop/Registry); Accounts (Identity); Distribution (Releases,
  Build Cache); Operations (Hosting, Deployments, Access, History). Unavailable
  sections remain visible with a clear planned/unavailable state.
- Use a compact header for current section, signed-in operator, and platform
  status. Do not show secrets, internal paths, or raw environment values.
- Establish reusable page primitives: section heading/actions, status and
  readiness badges, filter/search row, paginated table/list, selected-item
  detail panel, confirmation panel, operation progress/result, and consistent
  loading/empty/error/unauthorized states.
- Keep the design responsive, keyboard-operable, screen-reader labelled, and
  usable without JavaScript for navigation and read-only content. JavaScript
  may enhance filtering or polling but cannot be required to reveal the page.

### Overview and service workbenches

- Overview answers, at a glance: which services are healthy, which features are
  actually ready, what operation is pending/running/failed, and what requires
  the operator next. Health and feature readiness remain distinct.
- The Overview request queue is a read-only summary of owner API records. It
  may show counts and deep links, but the owner workflow remains the only place
  to inspect immutable request details or make decisions.
- Operations has separate Deployments, Hosting, Temporary Access, and History
  views. Every operation shows requester, exact revision/target, approval
  state, start/end time, result, and a bounded explanation; long-running work
  is tracked by operation ID and polled rather than blocking a request.
- Workshop/Catalog supports searchable records and a detail view for canonical
  identity, source/product type, local-source requirements, Workshop binding,
  visibility, and release/channel state. Publish/update remains unavailable
  until the owning publication authority exists.
- Identity supports searchable profiles, linked providers, role grants and
  revocations, permission requests, active/revoked sessions where available,
  and audit history. Mutations are previewed, confirmed, capability checked,
  and audited by Identity.
- Releases shows immutable bundle/manifest/target/signature/channel state with
  explicit validation failures and no secret material.
- Build Cache shows one current snapshot per target, freshness against the
  single supported Vapor/toolchain lock, builder status and last outcome. Only
  the builder workflow can replace a snapshot.

### Request history ownership contract

- Operations History is a read-only projection over bounded owner APIs. It
  must never query service databases or create a second cross-service ledger.
- Keep decision state separate from execution/dispatch state. For example,
  “approved · dispatch failed” remains searchable both as an approved request
  and as a dispatch failure.
- Request summaries show requester provenance from the owner API (Identity
  profile ID and request channel). The Platform shell does not join or query
  Identity storage to invent a display name.
- A source outage is local to that source and does not erase records returned
  by other services. Identify which service supplied each record and when its
  records were refreshed.
- Platform deployment and temporary-access APIs support stable keyset
  pagination with bounded pages up to 200 records. History loads additional
  Platform pages on demand. Identity still owns its own request-history
  contract. Platform deploy/restore outcomes are retained in the
  Platform-owned operation-history table and shown as a separately paginated
  History source; this does not create a cross-service request ledger.
- An Identity request can pass its expiry timestamp while its persisted status
  remains `pending`; the current expiry transition happens when an approval is
  attempted. The dashboard may show this as expired-by-time, but must not
  present it as an active approval or mutate Identity state. Identity still
  needs to decide and implement its durable expiry transition/audit behavior.

### Acceptance bar

- Every navigation entry lands on an implemented, useful screen or a clearly
  described unavailable state; there are no placeholder links disguised as
  working tools.
- Every implemented section has a task-oriented primary view, usable search or
  filtering when its data is a collection, and useful no-data/error states.
- A new operator can understand what a state means and what action is available
  without reading server runbooks first.
- A state-changing action always has an owning service, exact target, explicit
  consequence/confirmation, durable result, and owner-side audit record.
- A separate review pass checks the whole operator journey, service ownership,
  authentication boundaries, responsive behavior, and keyboard/accessibility
  behavior before the next deployment is treated as the dashboard release.
