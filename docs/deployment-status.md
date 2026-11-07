# Vapor server deployment status

## Directly verified live state (2026-11-07)

This section supersedes earlier status summaries below. The host was inspected
over root SSH, and local and public health checks were run after recovery.

- The approved deploy request
  `bdf86cf7-f6b5-4681-bc9d-ee644d2ca5a3` targeted `a8462945af4b16befd9e94964917cad15e33cfd6`.
  Approval persisted cleanly, but its deployment failed during startup. The
  request is terminal with `status=failed`; it was not re-used.
- Recovery exposed two startup defects: the shared Platform unit imported only
  `platform.env` even while configured for monolith mode, and the SSH audit
  trigger migration could fail when the trigger already existed during startup.
  The unit configuration and migration were corrected in commit
  `09ae2af441e55911c20087ed8922eb8cab01b8cb`.
- The Platform service was rebuilt from that exact commit on the VPS as a
  narrowly scoped recovery. It was running in `monolith` mode with zero
  restarts after recovery. The next approved request,
  `5338a3de-fd38-4c89-94ed-5afba23722ab`, targeted `95a8bbcdfb6cabcf21352fd25285c3eaf87638fe`.
  Dashboard dispatch failed twice with a generic helper error, so root started
  that exact approved request's one-shot worker directly. The worker built and
  deployed the approved commit successfully in the existing `monolith` mode.
  The full local health script then passed, including the public
  Atlas-through-Caddy route, and SQLite integrity returned `ok`.
- The request record is `status=succeeded`, while its dispatch substate remains
  `failed` because the dashboard helper did not queue it; the direct root start
  was not represented in that substate. This is a known record/presentation
  inconsistency, not a failed deployment. The helper's structured failure
  detail is currently discarded by the deployed Platform binary, so its exact
  cause is not yet established.
- A pre-repair SQLite backup is retained at
  `/var/backups/vapor-server/platform-pre-repair-20261107T010500Z.sqlite3`;
  the failed candidate binary is retained under
  `/var/backups/vapor-server/platform-failed-candidate-20261107T011200Z/`.
  The backup is protected as root-only state.
- GitHub Actions CI run `42323052997` for recovery commit `09ae2af` completed
  successfully. The live source checkout has since advanced to `95a8bbc` via
  the approved request above; the prior failed request remains recorded as
  failed.
- The previous successful deploy of `6df33ec` was on 2026-11-06. The earlier
  CI failure on `d5e01da` was a recursive-checkout failure because the pinned
  Identity commit was not fetchable from its remote at that time; the later
  successful recursive checkout proves that pin is now published and fetchable.
- The VPS contains retained `.tar.gz` state archives, but the live backup
  directory does not currently contain the new export's checksum sidecars,
  `index.json`, or `latest.json`. The new index/verification behavior is source
  only until the candidate is approved and deployed. The owner's USB was not
  connected during this check, so USB sync and restore remain unverified.

Last verified legacy baseline: 2026-07-25 Europe/Berlin via GitHub Actions
deploy run `30139280612` and CI run `30139280620`.

This records the last verified live VPS state, not the current source tree.
Changes made after that deployment are not considered deployed until a later
VPS check updates this record.

## Historical VPS baseline (verified 2026-07-25)

- OS: Ubuntu 26.04 LTS.
- Panel: none; Plesk is not used.
- Reverse proxy: Caddy.
- Service manager: systemd.
- Automatic deployment: `vapor-deploy.timer` checks the configured branch.
- GitHub deployment trigger is configured and verified through a restricted VPS
  deploy user. Branch protection is not configured yet.
- Intended public host: `vapor.ghf-studios.site`.
- Temporary pre-DNS access: HTTP fallback may be configured directly against the
  VPS while the domain registration is pending.
- Firewall: UFW enabled with inbound SSH, HTTP, and HTTPS only.
- Deploy root: `/opt/vapor-server-root`.
- State root: `/var/lib/vapor-server`.
- Server-local config: `/etc/vapor-server`.

## Recovery and cutover status (reported 2026-11-03)

This section supersedes the historical timer and deploy-root statements above
for current planning. It is based on the recent uploaded VPS reports and the
latest safe cutover attempt, not a fresh direct SSH inspection.

- The public site returned HTTP 200 and Caddy was active after its config
  permissions were repaired.
- The old periodic deploy timer had been disabled. The latest cutover retry
  stopped at preflight because the existing Platform checkout was not at the
  revision hard-coded into that script; it made no changes.
- `/opt/vapor-platform-server` is an existing Git checkout and must be treated
  as preserved state, not as an empty legacy directory. Keep its current
  source, state, service-local configuration, and Caddy configuration backed up
  through the next cutover.
- The request-triggered deployment flow and temporary SSH grant system are
  source work only until a new approved revision is installed and verified.
  No temporary SSH account or grant should be assumed to exist.
- The Platform source already composes Build Cache and exposes its admin
  inventory. The VPS-hosted Linux and Windows snapshot builder orchestration
  remains an outstanding infrastructure phase; the present Build Cache
  contract is not proof that either target snapshot is being produced.

An additional public read-only probe on 2026-11-03 returned `200 ok` for
`/healthz`, but `404` for `/admin/operations`, `/api/admin/operations`,
`/api/build-cache/v1/status`, and `/docs`. This is consistent with the new
composed Platform/admin routes not being live yet; the generic health route
alone does not identify the running revision. This probe did not access the
VPS directly and does not supersede the need for an authenticated host report.

The one-time cutover should therefore be an explicit migration from the
preserved Platform checkout to an exact approved root commit, retaining the
existing path and rollback copy until the dashboard, request-triggered deploy,
health checks, and Build Cache service all verify. It must not restore or enable
the retired polling timer.

Secrets are intentionally not recorded here. Admin tokens live only in
server-local env files under `/etc/vapor-server`.

## Recovery and optional worker direction (design, not deployed)

The intended recovery bundle is being changed from a state-only archive to a
consistent primary-host recovery set: service state, `/etc/vapor-server`
configuration and credentials, selected Caddy/systemd routing files, and exact
source/submodule pins. The archive will be intentionally unencrypted and copied
hourly to an owner-held 128 GB USB drive by a Windows-side SSH pull task. The
root SSH private key remains separate. Existing bundles do not satisfy this
target and must not be described as full-platform backups.

A future second Strato VPS is optional build and replica capacity. It will not
host public routes or own primary service state. Build/deploy and recovery must
remain usable when it is offline; its actual role depends on the selected
server's specifications. No worker or USB restore drill has yet been verified.

## Current source-only progress (2026-11-07)

The worktree now contains an independent-mode Platform admin gateway slice:
Platform remains on loopback port 7110, serves its own admin pages and health
route, and forwards only fixed Identity, Workshop, Artifact, and Build Cache
admin page prefixes to their owning service ports. The services-mode Caddy
template routes `/admin*` and `/api/admin*` to that gateway; service APIs stay
on their existing owner routes. The deployment path now builds, installs,
enables, restarts, and health-checks the gateway in services mode, and the
Platform unit reads only its own environment file. The deployment health check
also probes each fixed owner-admin page so a service returning healthy cannot
hide a broken shared-admin route.

The gateway implementation is present in the deployed binary, but the live
host still runs `monolith` mode; independent service routing and gateway
behavior have not been activated and runtime-verified. The gateway still uses
the existing shared HTML shell middleware for service pages; replacing that
HTML marker/buffering contract with the explicit shared-layout contract in the
approved design remains open. USB integrity revalidation is implemented in
source but has not run on the owner's drive. Fresh-host bootstrap/restore
remains open; the current recovery scripts are not a proven rebuild path.

The current local work also pins the deployment topology (`services` or
`monolith`) inside each immutable approval request. New CI requests target the
approved independent-services architecture; the dashboard displays the target
and refuses approval of legacy requests without one; the worker verifies and
deploys only the recorded target. The fixed helper failure reason is also
recorded in Platform logs when the helper returns a bounded structured error.
This slice has passed the Platform Rust binary tests and focused deploy-queue
tests locally; it is not yet pushed or deployed.

## Verified deployed source

These are the runtime/service commits verified on the VPS. Documentation-only
commits in `Vapor-Platform-Server` may be newer than the runtime-impacting commit
recorded here.

```text
Vapor-Platform-Server       a2a9592
Vapor-Homepage-Server   a41aedc4180792d5561a8e3bf12a1383e172c1ea
Vapor-Docs-Server       63ff610
Vapor-Identity-Server   ca9e61e
Vapor-Diagnostics-Server 058c403
```

## Verified runtime state (historical snapshot, 2026-07-25)

- `caddy.service` is active/running.
- `vapor-homepage.service` is active/running.
- `vapor-docs.service` is active/running.
- `vapor-identity.service` is active/running.
- `vapor-diagnostics.service` is active/running.
- `vapor-deploy.timer` is enabled for periodic deployment checks.
- The first timer-triggered `vapor-deploy.service` run completed successfully
  against `main`.
- Restricted deploy-user SSH can trigger `vapor-deploy.service`, and the
  resulting service run reports `success`.
- GitHub Actions deployment secrets are configured. Push-triggered workflow run
  `30126015161` successfully requested the VPS-owned deploy service, printed
  deploy service status, and ran public pre-DNS HTTP smoke checks.
- GitHub Actions workflow-dispatch run `30126858680` successfully verified the
  deployment path after automatic state-export timer installation.
- GitHub Actions push-triggered deploy run `30139280612` successfully deployed
  root commit `a2a9592`, printed deploy-service status, and ran public HTTP
  smoke checks. Companion CI run `30139280620` ran repository-local QA against
  recursive submodules.
- `vapor-state-export.timer` is enabled/active. It produced
  `/var/backups/vapor-server/vapor-server-state-20260724T211010Z-3ec87f6.tar.gz`
  with root-only permissions. The bundle manifest reports
  `secrets_included = false` and excludes `/etc/vapor-server`.
- Local health/status checks pass on ports 7111, 7112, 7113, and 7114.
- Public pre-DNS HTTP health/status checks pass through the fallback route.
- `/etc/vapor-server/root.env` preserves non-secret deployment settings for
  timer-driven runs, including the temporary pre-DNS HTTP fallback host.
- SSH remains reachable through key authentication after hardening.
- Identity uses SQLite and has been initialized through the local admin-token
  endpoint.
- Identity database files are owned by `vapor:vapor` with restrictive file
  permissions.
- Identity auth readiness endpoint is deployed. GitHub OAuth browser/device
  configuration is present and reports ready. Steam Web API key configuration is
  present and reports ready.
- Top-level `/login`, `/logout`, and `/admin` routes are routed to the identity
  service. `/login` is the browser login/register page. `/admin` is publicly
  reachable as a locked shell.
- Identity has Steam-anchored browser profiles through Steam OpenID, GitHub
  browser OAuth linking stubs, GitHub Device Flow/API seams, 5-minute auth
  attempts, and 5-minute dashboard sessions. The dashboard only renders
  privileged identity data/actions when the request carries a non-expired root
  session for a profile with linked Steam and GitHub identities plus the `root`
  role.
- Identity source has been split out of the previous monolithic `main.rs` into
  focused modules for config, route handlers, persistence, provider
  verification, profile/session logic, and shared utilities.
- Identity has a server-local role-grant operator route and script:
  `POST /v1/admin/roles/grant` and
  `deploy/scripts/grant-identity-role.sh`. The route grants `root` or
  `content-developer` only by requiring both external identities: SteamID64 and
  GitHub login. Those identities must already be linked to the same internal
  profile row. The internal profile id is not accepted as grant authority. The
  route accepts either the server-local bootstrap token or a non-expired root
  dashboard session.
- Identity has role-revoke routes and scripts:
  `POST /v1/admin/roles/revoke`, `POST /admin/identity/roles/revoke`, and
  `deploy/scripts/revoke-identity-role.sh`. Revocation uses the same linked
  SteamID64 plus GitHub-login target model as grants. The service refuses to
  revoke the last active `root` role and rejects attempts to revoke
  `content-developer` from an active root profile because `root` implies
  developer capability.
- Identity has recent audit visibility through `GET /v1/admin/audit`, the root
  dashboard, and `deploy/scripts/list-identity-audit.sh`. Authenticated VPS
  smoke verified the audit helper works without printing the server-local admin
  token.
- The root dashboard has first functional role-grant and role-revoke controls
  backed by the same validation paths as the JSON API. Local tests cover root
  effective-role display, last-root revoke refusal, no-op revoke audit behavior,
  and audit listings that avoid exposing internal profile ids.
- Internal profile ids are not shown in the browser dashboard or protected
  profile listing; they remain database join keys only.
- Public unauthenticated requests to role-grant, role-revoke, and audit routes
  reject with `401`.
- Public docs and diagnostics status probes are deployed:
  `/docs/status` and `/api/diagnostics/status`.
- Public unauthenticated contract checks cover docs write/export rejection,
  diagnostics protected read rejection, identity admin rejection, and the
  removed legacy root-grant route.
- The removed `/v1/admin/root/grant` compatibility route returns `404`.
- `deploy/scripts/configure-identity-auth.sh --status` waits for the identity
  service after restart instead of immediately failing on a bind/readiness race.
- `root` is a role/group on normal Steam-anchored profiles, not a separate
  account type. GitHub does not create standalone player profiles. Effective
  authorization treats `root` as implying `content-developer`.
- The old server-local dashboard password remains present in
  `/etc/vapor-server/identity.env` for compatibility/readiness visibility, but
  it is no longer the dashboard authorization model.
- Temporary HTTP-by-IP identity cookies are explicitly configured with
  `VAPOR_IDENTITY_PUBLIC_ORIGIN=http://82.165.77.104`,
  `VAPOR_IDENTITY_COOKIE_SECURE=false`, and `VAPOR_IDENTITY_COOKIE_PATH=/`.
  Move this to secure HTTPS cookies once DNS is active.
- `deploy/scripts/configure-identity-auth.sh` and
  `deploy/scripts/smoke-identity-auth.sh` are deployed on the VPS.
- Curated Vapor docs are deployed through the public HTTP docs route: 410 files,
  8,739,662 bytes uncompressed, served under `/docs/`.
- Docs source now uses a single current-tree publication model. Existing
  deployed docs state may still be the previously uploaded curated docs bundle
  until the next authenticated docs publication.
- Diagnostics source now exposes one structured JSON report API with stronger
  redaction, collision-resistant run ids, and aggregate quota support. Public
  smoke verifies protected reads fail closed without authorization; an
  authenticated diagnostics upload smoke has not been run on the VPS in this
  verification pass.
- An older diagnostics smoke run was previously uploaded and verified to redact
  obvious secret tokens on disk.
- A root-only state export bundle was created under `/var/backups/vapor-server`
  and verified to contain `/var/lib/vapor-server` state plus a manifest, while
  excluding `/etc/vapor-server` env/token files.
- Restore source now validates state-bundle structure before extraction and
  before stopping services. A disposable-server restore rehearsal remains
  pending.

## Remaining external dependency

DNS for `vapor.ghf-studios.site` still needs to point at the VPS once the domain
registration is active. Public HTTPS/certificate issuance depends on that DNS
being correct.

The temporary HTTP fallback should be removed after DNS and HTTPS are verified.
