# Vapor server decisions and backlog

This file records server-side decisions and open work that should stay visible
while the implementation is still moving quickly.

## Accepted boundaries

- `Vapor-Platform-Server` lives beside Vapor Client; it is not a Vapor Client
  submodule.
- Future server domain ownership is tracked in
  `docs/server-domain-boundaries.md`; that document should be kept aligned with
  this accepted-boundaries list as new service boundaries become real.
- `Vapor-Platform-Server` owns deployment/orchestration, shared operations
  conventions, and whole-system export/import composition.
- Homepage, docs, identity, and diagnostics remain separate repositories and
  separate service binaries.
- A single public domain should be sufficient for now, with path routing:
  `/`, `/docs/`, `/api/identity/`, `/api/diagnostics/`, `/api/registry/`, and
  `/api/updates/v1/vapor/`; operator sections live under `/admin/*`.
- Service dependencies are allowed when they reduce avoidable complexity.
  The initial scaffolds use Axum/Tokio instead of hand-rolled HTTP parsing.
- Diagnostics upload is explicit opt-in only. There is no default/public
  telemetry goal.
- Diagnostics should not collect hostnames or persistent machine identifiers as
  a product requirement. Rough system/platform/spec information is preferable
  when diagnostics context is useful.
- Git is not the normal diagnostics transport and should not be a normal player
  dependency.
- Players should not require GitHub accounts.
- Developer source workflows may require a linked GitHub identity for
  repository access and attribution. Public content consumption must not
  require GitHub merely because a source repository exists; the content's
  acquisition provider and license decide availability.
- The Steam account is the primary Vapor player profile anchor. GitHub attaches
  to a Steam-anchored profile for development/root authorization; it should not
  create a standalone player profile.
- `root` is a privileged role/group on normal Steam-anchored profiles, not a
  distinct account type.
- Steam browser identity, Steamworks/WebAPI verification, Vapor roles, and
  publishing authority are separate layers. OpenID is sufficient for browser
  identity, but developer/root publishing paths need stronger Steam/Vapor
  authority checks.
- Identity/auth concerns belong on the identity side. Other services should use
  identity-issued authorization instead of inventing incompatible auth
  systems. Registry's first protected ecosystem lookup now uses the internal
  Identity decision route; every future protected resource route must adopt the
  same service-owned-policy boundary.
- Registry is the target owner of Vapor's provider-neutral Projects and content
  catalog: Project metadata, typed content identity, immutable releases,
  acquisition bindings, discovery, and runnable compositions. A Project is the
  workspace; a composition selects releases and prerequisites into a runnable
  setup. Engine, Game, Packagepack, and Mod are content roles, not synonyms for
  Project or composition. Steam Workshop is one acquisition/distribution
  provider; its IDs and tags do not define Vapor content types. Identity owns
  account and Project-access decisions, not catalog records. Current
  Registry storage is only provider/repository/ecosystem source topology; the
  typed catalog and its read/write lifecycle do not exist yet.
- Server state must be restorable service-by-service and as a composed whole:
  empty initialization, export, import, and rebuild-from-source are first-class
  operational goals.
- The current deployment direction is direct Linux service hosting with Caddy
  and systemd. Plesk is not required and is not part of the Vapor server MVP.
- If deploying without Plesk, Ubuntu 26.04 LTS is an acceptable baseline. If
  deploying with Plesk, use the latest Plesk-supported Ubuntu LTS instead of
  Ubuntu 26.04 until Plesk support catches up.
- The ordered domain is `ghf-studios.site`; the intended Vapor service host is
  `vapor.ghf-studios.site`.
- Identity must be database-first. Do not base real identity state on an ad-hoc
  filesystem registry file.
- SQLite is acceptable for the initial single-VPS vertical slice when used as a
  real database: migrations, restrictive ownership/permissions, WAL mode,
  backups/export, and service-owned schema management.
- PostgreSQL remains the likely migration target if the server grows beyond a
  single VPS, needs multi-node operation, or has concurrency/operations needs
  that justify the extra service.
- Automatic deployment should follow a real branch-based path. The initial
  target branch is `main` unless a dedicated deployment branch is introduced.
- GitHub-triggered deployment may submit an exact tested revision using a
  request-only Identity credential; it cannot approve or execute deployment.
  A root operator approves through the same-origin Platform dashboard, which
  dispatches a request-specific VPS-owned unit. GitHub Actions has no SSH key.

## Current scaffold behavior

- Homepage serves public homepage/legal placeholder routes and health.
- Docs serves health, token-protected current-docs upload, archive upload, and
  token-protected export scaffolds. It intentionally owns only the currently
  served docs tree in this pre-alpha slice.
- Identity serves health/status plus token-protected init/export scaffolds
  backed by SQLite/SQLx schema bootstrap. It has fail-closed Steam Web API
  ticket verification, Steam OpenID browser login, GitHub OAuth token
  verification, GitHub Device Flow, GitHub browser OAuth linking, and a
  short-lived dashboard session model. The Platform-owned `/admin` route renders
  the shared toolbench shell; Identity's service-owned `/admin/identity` section
  renders privileged identity data/actions only for a non-expired root profile
  session with linked Steam and GitHub identities. The former standalone
  Identity `/admin` route is removed with no compatibility alias. The server-local
  admin token remains an operations bootstrap tool, not the normal dashboard
  login model. Role grants require both SteamID64 and GitHub login, reject
  internal profile ids as authority, and can be authorized by either the
  bootstrap token or a root dashboard session. Role revocation now uses the same
  external-identity target model, refuses to revoke the last active `root`, and
  records audit events visible to root/admin operators.
- Diagnostics accepts unauthenticated upload scaffolds and keeps list/download/
  export behind an admin token for now. Source now includes one structured JSON
  report route with explicit consent, allowlisted text artifacts,
  collision-resistant run ids, stronger redaction, and a non-destructive
  aggregate storage quota.
- `Vapor-Platform-Server` tracks the services as root-level submodules named after
  their repositories. Normal local development should use those submodule
  worktrees, not separate sibling checkouts.
- `Vapor-Platform-Server` now contains a source-level, human-approved operations
  path: CI submits an exact root and submodule revision, root approval dispatches
  only that request's systemd unit, and temporary SSH access uses a separate
  account, reviewed key fingerprint, bounded capability set, and expiry cleanup.
  The older five-minute deploy poller and GitHub Actions SSH handoff are retired.
  These operations are not considered live until the guarded VPS cutover and
  dashboard checks are completed. Existing state export/import, Caddy/systemd
  deployment, health checks, and operation result reporting remain part of the
  same Platform-owned operational spine.
- Platform owns the `/admin` shell, overview, and human-approved Operations
  section for deployment and temporary SSH requests;
  Identity, Registry, and Artifact own their protected sections and data.
  Build Cache now exists as a separate service and its state is composed into
  deploy/recovery. Native build operations, optional worker execution, and
  Vapor Client snapshot bootstrap remain unimplemented; do not advertise usable snapshots
  until those pieces and exact target lock identities are configured. The
  current accepted contract allows local builds as the correctness fallback and
  an optional second worker as an accelerator. Do not treat the superseded plan
  to host both target VMs on the primary VPS as active work.
- Domain-independent pre-DNS work is allowed through temporary HTTP fallback
  routing, SSH/UFW hardening, and local/IP smoke checks. The fallback is not the
  final public product URL.

## Near-term backlog

- Finish the standalone Vapor release/client integration: publish a signed
  Windows release, embed and rotate the production verification key, and connect
  developer-package repository dependencies to lazy Superworkspace acquisition.
  Keep source checkouts separate from the app install and preserve both the
  Superworkspace and locally mounted game content on update/uninstall.
- Review and run the VPS provisioning model around direct Caddy/systemd
  deployment.
- Wire DNS for `vapor.ghf-studios.site` once the domain is active.
- Finalize the Caddy route configuration for `vapor.ghf-studios.site`.
- Remove the pre-DNS IP/HTTP fallback once DNS and HTTPS are verified.
- Exercise restore/import on a fresh or disposable server instance, including
  bundles produced by the automatic state-export timer.
- Add service-owned semantic export/import validation after the root-level
  structural bundle validator has been exercised.
- Add SQLite migration/bootstrap handling for identity and any service-owned
  state that should be queryable.
- Evolve export/import bundle formats for docs, identity, and diagnostics beyond
  the initial root-level file-state bundle.
- Deploy and verify the simplified Docs current-publication source slice, then
  decide whether the next docs step is identity-root authorization, richer docs
  publishing UX, or a service-owned streamed export/import format.
- Decide whether the next diagnostics step is richer operator listing/download
  UX, identity-root authorization, or an explicit service-owned export/import
  bundle.
- Move docs, diagnostics list/download/export, and other privileged service
  APIs from placeholder tokens/admin tokens toward identity-root authorization
  using the contract in `docs/cross-service-authorization-contract.md`.
- Create the future content-upload service boundary. At creation time, assign
  the authenticated Identity profile as canonical creator; treat client-sent
  author labels as display metadata, and check stored ownership for update and
  deletion. Upload, moderation, and owner-management routes are not implemented
  by the current Registry slice.
- Exercise the Steamworks/Vapor-client ticket path for end-to-end developer auth
  beyond browser OpenID login: client obtains a Steam Web API ticket, server
  verifies it, and developer workflows can rely on the stronger proof.
- Define the secure publishing authority model for Workshop publishing and
  root app/server publishing. Root/admin implies developer capability, but
  Steam-side publishing still needs appropriate Steamworks/pipeline authority.
- Move pre-DNS HTTP cookie config to HTTPS-only secure cookies once DNS and
  certificate issuance are live.
- Improve dashboard role-management UX beyond the first functional grant/revoke
  forms and audit table.
- Implement durable Platform build-operation records and the optional worker
  lifecycle only after the canonical lock/recipe inputs are available. Keep
  local source builds usable without the worker; keep shared `sccache` data
  separate from Build Cache target snapshots. The Build Cache service/admin
  section already exists, but neither is evidence that a native builder exists.
- Decide how Vapor Shell should wrap server REST APIs without making raw HTTP
  details the normal user workflow.
- Configure GitHub branch protection once the exact protected-branch policy is
  accepted.

## Explicitly not current scope

- Plesk setup.
- Server-mediated Steam publishing credentials.
- A launcher GUI. The shared read-only Platform admin toolbench is accepted;
  do not fold service business logic into the Platform shell.
- MCP/ACP capability integration.
- Public telemetry.
- Git-backed diagnostics transport.

## Open questions

- Should diagnostics list/download stay strictly root-only from day one, or can
  early pre-alpha tolerate broader temporary access?
- What is the minimal identity model needed before docs and diagnostics should
  consume identity-issued authorization?
- Should docs, diagnostics, and identity state exports be separate files inside
  one root bundle, or one service-owned bundle per service plus a root manifest?
- How much hardware/system information is useful in diagnostics without turning
  the upload into fingerprinting?
- Which parts of this deployment belong in Vapor Shell as operator commands, and
  which should remain server-root-only scripts?
