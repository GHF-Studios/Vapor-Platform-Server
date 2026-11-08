# Vapor server vertical-slice contracts

This file describes the current working slice as an explicit set of contracts.
It is intentionally stricter than the current implementation polish: when code
and this document disagree, treat that as a bug or an intentional migration that
must be documented.

## Goal of the current slice

The server stack should prove the full path from public web entrypoint to
service state and back:

```text
browser/Vapor client
  -> one public HTTP(S) origin
  -> Caddy path router
  -> independently owned service binaries
  -> service-owned state under /var/lib/vapor-server
  -> export/import/rebuild operations from Vapor-Platform-Server
```

The goal is not a polished production frontend yet. The goal is that every
working behavior has a clear owner, a clear authority model, and a clear upgrade
path.

## Repository ownership

- `Vapor-Platform-Server` owns orchestration: Caddy, systemd, deploy scripts,
  bootstrap/hardening, whole-system export/import composition, and operator
  runbooks.
- `Vapor-Homepage-Server` owns public homepage/legal/product pages.
- `Vapor-Docs-Server` owns docs artifact upload, serving, current/version
  pointers, and docs export/import.
- `Vapor-Identity-Server` owns Steam identity, GitHub identity, roles, sessions,
  and authorization decisions.
- `Vapor-Diagnostics-Server` owns opt-in diagnostics uploads, redaction,
  indexes, retention, list/download/export, and diagnostics restore/import.
- `Vapor-Artifact-Server` owns immutable public Vapor application release
  manifests, signatures, archives, and the associated read routes. Release
  signing remains offline; the service does not accept public uploads.
- `Vapor-Build-Cache-Server` owns signed current Cargo target snapshots,
  candidate validation/promotion, target availability, and its read-only admin
  inventory. Platform composes its routes and deployment/recovery only.

The root repo may provide shared deployment conventions. It must not absorb
service business logic unless a shared library is deliberately introduced.

## Route map

```text
/                         public homepage
/docs/                    public current docs
/login                    public Steam-anchored login/register
/login/steam              Steam OpenID browser auth
/login/github             GitHub browser link flow after Steam login
/logout                   identity session logout
/admin                    public shell; privileged content requires root session
/admin/operations         Platform-owned latest deployment/recovery status; root session only
/admin/identity           Identity-owned dashboard; root session only
/admin/identity/roles/grant browser role-grant form; root session only
/admin/identity/roles/revoke browser role-revoke form; root session only
/admin/registry           Registry-owned ecosystem/source inventory; root session only
/admin/artifacts          Artifact-owned read-only release inventory; root session only
/admin/build-cache        Build Cache-owned read-only target inventory; root session only
/api/admin/overview       root-authorized, secret-free service health summary
/api/admin/operations     root-authorized latest deploy/restore status record
/api/identity/            identity service API
/api/diagnostics/         diagnostics service API
/api/registry/            Registry ecosystem/source-topology API (not the planned content catalog)
/api/updates/v1/vapor/    immutable Artifact release API
/api/build-cache/         current signed target snapshot API
```

Every stateful service should expose a public, secret-free status probe
that reports readiness and temporary auth model without leaking tokens.

Protected admin sections authorize the Identity session at the owning service
handler. The shared shell and navigation do not grant access. Registry and
Artifact use distinct audience-bound server tokens; release inventory reports
metadata and file presence but does not perform cryptographic verification or
publication.

Platform Operations displays only the latest validated deploy/restore status
record and links to operator runbooks. It is read-only and is not an
append-only audit log.

Temporary pre-DNS HTTP-by-IP access exists only for the current setup phase. The
intended long-lived public origin is `https://vapor.ghf-studios.site`.

## Identity model

- Steam is the Vapor profile anchor.
- GitHub attaches to a Steam-anchored profile for development/root work.
- Players do not need GitHub.
- Developers need Steam + GitHub.
- `root` is a role on a normal Steam-anchored profile.
- `root` implies `content-developer` capability.
- The internal profile row exists to join Steam accounts, GitHub accounts,
  sessions, roles, and audit events. It is not a login credential and should not
  be exposed as the user-facing authority.

## Authorization contract

| Capability | Required authority |
| --- | --- |
| View public homepage/docs | none |
| Publish docs | docs upload token for now; future root/pipeline auth |
| Upload diagnostics | explicit opt-in structured JSON report upload |
| List/download diagnostics | diagnostics admin token for now; future identity-root session |
| Create Steam player profile | Steam OpenID browser proof |
| Link GitHub | existing Steam session + GitHub OAuth proof |
| View admin dashboard data | non-expired root dashboard session |
| Grant roles from dashboard | non-expired root dashboard session |
| Revoke roles from dashboard | non-expired root dashboard session; last active root is protected |
| View identity audit events | server-local admin token or non-expired root dashboard session |
| Bootstrap first root | server-local admin token or first-root flow |
| Emergency operator role grant | server-local admin token on the VPS |

Server-local admin tokens are operational bootstrap tools. They should not be
the normal UI or developer workflow after a root profile exists.

The target replacement for service-local Docs/Diagnostics read/write tokens is
the audience-bound introspection model in
`docs/cross-service-authorization-contract.md`. That contract is not deployed
yet; services must fail closed until they intentionally consume it.

## Role-grant contract

Role grants require both external identities:

```json
{"role":"root","steam_id64":"7656119...","github_login":"example"}
```

The server must verify that the SteamID64 and GitHub login are already linked to
the same internal profile row before writing the role. Role grants by internal
profile id are intentionally not supported.

Role revocation uses the same external-identity target shape:

```json
{"role":"root","steam_id64":"7656119...","github_login":"example"}
```

The service refuses to revoke the last active `root` role through normal
operator routes. Since `root` implies `content-developer`, revoking
`content-developer` from an active root profile is rejected; demotion to
developer is represented as explicitly granting `content-developer`, then
revoking `root`.

Audit listings identify actor/subject profiles by linked external identities.
They must not expose internal profile ids as operator-facing authority.

## State contract

```text
/var/lib/vapor-server/homepage       mostly stateless today
/var/lib/vapor-server/docs           docs artifacts and current pointers
/var/lib/vapor-server/identity       SQLite identity DB and session state
/var/lib/vapor-server/diagnostics    uploaded diagnostics and indexes
/var/lib/vapor-server/registry       Workshop/catalog metadata
/var/lib/vapor-server/updates        immutable application releases
/var/lib/vapor-server/build-cache   current target snapshots and candidate staging
/var/lib/vapor-server/operations     latest deploy/restore status pointer
/etc/vapor-server                    server-local env/secrets; included only in the new, intentionally unencrypted recovery-bundle design
```

State must be recoverable through:

1. rebuild source from the configured branch;
2. initialize empty service state; or
3. restore a previously exported state bundle.

Whole-system export/import is composed in `Vapor-Platform-Server`; service-specific
formats should become explicit service contracts over time.

The deployed-slice backup statements below describe the earlier state-only
bundle and are superseded by the recovery contract in
`docs/operations-evidence-and-recovery-contract.md`. The current source change
introduces schema-v2 full-platform bundles that include server configuration
and credentials in plaintext, selected host files, and source pins. This new
format is not yet deployed or proven by a clean-host restore drill.

The schema-v2 validator rejects path traversal, absolute paths, links/special
files, unsupported manifest versions, duplicate entries, incomplete source
pins, and any manifest that does not explicitly declare plaintext credentials.
See `docs/operations-evidence-and-recovery-contract.md`.

Diagnostics source now defines the current report contract for structured,
explicit opt-in JSON uploads. The storage shape is:

```text
/var/lib/vapor-server/diagnostics/runs/
  diag-<unix-milliseconds>-<uuid-v4>/
    metadata.json
    metadata.toml
    vapor.log
```

See `docs/diagnostics-report-contract.md` for schema, redaction, quota, and
privacy rules. This source-level contract should not be read as a live VPS
deployment claim until `docs/deployment-status.md` is updated after verification.

Docs source now defines a single current documentation tree. The source-level
state shape is:

```text
/var/lib/vapor-server/docs/
  current/
    index.html
    metadata.toml
    ...
```

See `docs/docs-publication-contract.md` for publication, validation, and export
semantics. This is likewise not a live VPS deployment claim until verified and
recorded in `docs/deployment-status.md`.

## Deployment contract

The current deployment path is:

```text
GitHub main branch
  -> CI verifies root plus every pinned service submodule
  -> Identity-scoped request for the exact tested revision
  -> root operator reviews and approves that ticket in Platform Operations
  -> one request-specific systemd deploy unit
  -> exact checkout, release build, independent service restarts, Caddy update
  -> local health checks and operation result
```

GitHub Actions creates a pending request after CI. Root approval dispatches the
request-specific systemd unit immediately; no deploy timer polls `main`. The
independent state-export timer remains. Server secrets remain on the VPS, not in
GitHub Actions. This describes the target source contract; the current VPS
cutover and dashboard state is tracked separately in
`docs/deployment-status.md`.

## Smoke-check contract

Minimum checks before claiming the stack is good:

- all four local service health checks pass;
- docs, identity, and diagnostics status probes pass;
- public pre-DNS routes answer while DNS is pending;
- `/api/identity/v1/auth/status` reports configured provider readiness
  accurately;
- unauthenticated role-grant attempts return `401`;
- unauthenticated role-revoke and audit-list attempts return `401`;
- removed legacy routes return `404`;
- profile listings do not expose internal profile ids;
- diagnostics smoke upload redacts obvious secrets;
- diagnostics smoke upload accepts structured explicit-consent JSON, stores
  redacted output, and does not echo distinctive fake secret values;
- docs route serves the current docs bundle;
- docs smoke upload replaces the current tree and status reports that current
  docs are ready;
- state export excludes `/etc/vapor-server` secrets;
- restore rejects structurally unsafe state bundles before extraction;
- automatic state-export timer is enabled and active.

## Current wobbly seams

These are not reasons to stop; they are the next places to widen the pipe.

- Admin UI is functional but unstyled and still missing role-change
  confirmation polish.
- Identity sessions are short and server-local; no JWT/service-to-service auth
  contract exists yet.
- Docs and diagnostics still use token/admin-token scaffolds rather than
  identity-root authorization.
- Docs current-tree publication exists in source; protected publication still
  uses the token scaffold until identity/pipeline authorization is implemented.
- Diagnostics schema, quota, and redaction source contract exists; identity
  authorization for diagnostics read/export is still pending.
- Restore/import has a root-level file-state path but service-owned import
  formats are not mature.
- Deployment is direct and useful, but branch protection/GitHub trigger secrets
  are not fully configured.
- HTTPS/domain cutover is pending DNS.
- Publishing authority is documented conceptually but not implemented as a
  protected pipeline.
