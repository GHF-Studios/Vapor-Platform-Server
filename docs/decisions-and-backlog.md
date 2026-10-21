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
  `/`, `/docs/`, `/api/identity/`, and `/api/diagnostics/`.
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
- Content/development workflows may require both Steam identity and GitHub
  identity because Vapor content is GitHub-backed for now.
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
  identity-issued authorization later instead of inventing incompatible auth
  systems.
- Registry owns the canonical Vapor Workshop catalog: typed content identity,
  content-to-Workshop bindings, discovery metadata, and version/channel records.
  Steam tags are discovery hints, not enforced Vapor types. Identity supplies
  verified author and role authorization facts; it does not own catalog data.
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
- GitHub-triggered deployment should trigger the VPS-owned deploy service rather
  than moving server secrets into GitHub Actions. A dedicated restricted deploy
  SSH user is preferred over using the root SSH key.

## Current scaffold behavior

- Homepage serves public homepage/legal placeholder routes and health.
- Docs serves health, token-protected current-docs upload, archive upload, and
  token-protected export scaffolds. It intentionally owns only the currently
  served docs tree in this pre-alpha slice.
- Identity serves health/status plus token-protected init/export scaffolds
  backed by SQLite/SQLx schema bootstrap. It has fail-closed Steam Web API
  ticket verification, Steam OpenID browser login, GitHub OAuth token
  verification, GitHub Device Flow, GitHub browser OAuth linking, and a
  short-lived dashboard session model. The admin dashboard is publicly reachable
  as a shell but only renders privileged identity data/actions for a non-expired
  root profile session with linked Steam and GitHub identities. The server-local
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
- `Vapor-Platform-Server` now contains first-pass direct VPS deployment automation:
  Ubuntu bootstrap, root-repo deploy, Caddy config installation, systemd unit
  installation, automatic branch polling via systemd timer, GitHub Actions
  deploy triggering through a restricted VPS user, automatic state-export
  backups, public HTTP smoke checks, and local health checks. It also contains
  first-pass whole-server file-state export/restore scripts for
  `/var/lib/vapor-server`, excluding server-local env/token files under
  `/etc/vapor-server`. Restore now validates bundle structure before extraction
  and before stopping services.
- Domain-independent pre-DNS work is allowed through temporary HTTP fallback
  routing, SSH/UFW hardening, and local/IP smoke checks. The fallback is not the
  final public product URL.

## Near-term backlog

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
  once `docs/cross-service-authorization-contract.md` is implemented.
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
- Decide how Vapor Shell should wrap server REST APIs without making raw HTTP
  details the normal user workflow.
- Configure GitHub branch protection once the exact protected-branch policy is
  accepted.

## Explicitly not current scope

- Plesk setup.
- Server-mediated Steam publishing credentials.
- A dashboard or launcher GUI.
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
