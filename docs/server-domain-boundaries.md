# Vapor server domain boundaries

Status: decision record for future server ownership. This is not an
implementation design.

## Purpose

Vapor server work should keep authority, state ownership, and public routing
clear as the current vertical slice grows. Current repository docs remain the
authority for implemented state. Anything beyond those docs is planning or
speculation and should be labeled that way.

The standing rule is: do not collapse unrelated responsibilities into an
existing service just to avoid naming a new boundary.

## Current implemented baseline

- `Vapor-Platform-Server` owns deployment/orchestration, reverse-proxy routing,
  systemd units/timers, operator scripts, and whole-system export/import
  composition.
- `Vapor-Homepage-Server` owns the public homepage/legal/product surface.
- `Vapor-Docs-Server` owns docs bundle upload, docs serving, current/version
  pointers, and docs export/import.
- `Vapor-Identity-Server` owns Steam-anchored profiles, GitHub linking, roles,
  sessions, identity audit, and authorization decisions.
- `Vapor-Diagnostics-Server` owns explicit opt-in diagnostics upload, redaction,
  diagnostics storage/indexes, retention direction, list/download/export, and
  diagnostics restore/import.
- `Vapor-Build-Cache-Server` owns signed current Cargo target snapshots,
  candidate validation/promotion, and target snapshot status. Platform owns
  deployment and recovery composition; target VM orchestration remains pending.
- One public origin with path routing remains the preferred shape.
- MCP/ACP/capability work is future work and is not part of the initial server
  MVP.

## Accepted Workshop catalog authority

The canonical Vapor catalog belongs to the future Registry service. Registry
will own typed content identity and type, the binding between Vapor content
IDs and Steam Workshop items, discoverability metadata, and version/channel
records. Steam Workshop remains the transport and distribution provider; its
tags and free-form descriptions are not authoritative Vapor content types.

Identity remains the canonical source of people, linked Steam/GitHub accounts,
roles, and authorization facts. Registry asks Identity (or consumes
Identity-issued authorization) when a catalog write requires a verified
author or elevated role. Identity does not own catalog records, and Registry
does not copy or redefine user roles.

The Registry API and persistent catalog are implemented. Its Workshop admin
section currently provides a bounded, read-only inventory; client-side
Workshop tags remain discovery hints and are not enforced Vapor semantics.

## Boundary map

| Domain | Boundary | Responsibilities | Non-responsibilities |
| --- | --- | --- | --- |
| Homepage | Implemented service | Public `/`, product/legal/home pages, public health | Login, docs, diagnostics, admin, publishing |
| Docs | Implemented service with temporary auth scaffolds | `/docs/`, docs artifacts, current/version pointers, docs export/import | Generic artifact storage, identity decisions, catalog |
| Identity | Implemented service and canonical auth owner | Steam profiles, GitHub links, roles, sessions, root/developer authorization, identity audit | Publishing execution, deployment, diagnostics/docs storage |
| Diagnostics | Implemented scaffold, policy incomplete | Opt-in upload, redaction, storage, indexes, retention, root-dev list/download/export | Telemetry, Git transport, hostnames, persistent machine IDs, identity authority |
| Publish/pipeline | Future real boundary | Protected publish requests, validation/promotion orchestration, publishing audit, controlled Steam/Workshop or server-publish authority | Identity source of truth, artifact blob semantics, public catalog reads |
| Artifacts | Implemented release service | Immutable Vapor application archives, signed manifests, channel pointers, public downloads, root-gated read-only release inventory | Catalog metadata, publish approval, signing keys, docs bundles |
| Build Cache | Implemented service; builder incomplete | Signed current target snapshots, candidate validation/promotion, read-only target inventory | Guest VM orchestration, Vapor Client restore/bootstrap, signing key custody |
| Registry/catalog | Implemented service | Public ecosystem metadata, source topology, visibility policy, root-gated read-only Workshop inventory | Blob storage, builds, Identity roles, deploy orchestration |
| Toolchain/API UX | Future integration boundary | Stable CLI/Shell/launcher workflows over existing APIs | Canonical state, role decisions, publish execution |
| MCP/capability surface | Future capability boundary | Machine/action capabilities, least-privilege grants, capability audit | Business state, bypassing service APIs |
| Operations/recovery | Implemented root-owned workflow plus service contracts | Backups, restore/import composition, deploy health, latest deploy/restore status, emergency operator paths | Normal product workflows, service business decisions |
| Cross-domain admin UI | Implemented shared shell | `/admin` navigation and read-only Platform overview/Operations; Identity, Registry, and Artifact own protected sections | Replacing domain services or storing canonical domain state |

## Admin authority, request lifecycle, and history

The shared admin UI is a presentation and navigation layer. It does not decide
who may perform an action, own another service's request state, or create a
second audit ledger. Each service owns its request lifecycle and exposes only
the records its protected admin API intentionally returns.

| Request or operation | State owner | Who decides | Who executes | History source |
| --- | --- | --- | --- | --- |
| Platform deployment request | Platform | Root Identity session approves/rejects the exact pinned revision | Platform's fixed privileged helper and request-scoped systemd worker | Platform deploy-request API; request status, dispatch stage, and result are distinct |
| Temporary SSH access | Platform | Root Identity session chooses the actual capability and duration or rejects | Platform's root-owned typed helper provisions/revokes the restricted account; expiry timer enforces the grant | Platform SSH-request API and per-request transition events |
| Identity role permission request | Identity | Root Identity session approves/rejects the requested grant/revoke | Identity applies the role transition | Identity permission-request API and Identity audit log |
| Catalog, release, and build-cache administration | Registry, Artifact, and Build Cache respectively | The owning service's authorized policy | The owning service's audited operation, when implemented | Owner API; no Platform database reads |
| Shared navigation, service health, and attention counts | Platform shell | No authority decision | No mutation | Explicit owner health and protected request-list APIs; no service DB reads |

The Operations **History** view is a bounded, read-only projection over the
Platform deployment and temporary-access APIs plus Identity's permission
request API. The service API that owns a record remains authoritative; partial
service outages are reported per source. The view does not write records or
aggregate service databases. Each source currently returns at most its most
recent 200 requests. Per-request temporary-access transitions are fetched from
the Platform event endpoint when expanded.

`/var/lib/vapor-server/operations/latest.json` is only the latest deploy/restore
status pointer. It is not an operation event log and must never be presented as
complete history. Durable multi-entry Platform deploy/restore operation
history, a shared cursor/pagination contract across services, and a common
cross-service audit-event schema remain open work. They require owner-service
contracts first; they do not justify a central database reader or moving audit
ownership into the Platform shell.

The bounded request APIs expose requester provenance as an opaque Identity
profile ID plus whether the request came through a request credential or an
interactive session. The UI may display and search those fields, but must not
resolve another service's profile database itself. Human-readable requester
labels require a deliberate Identity-owned directory/summary API and remain
open work.

Identity permission requests have a special current-state edge: the approval
handler persists `expired` when an expired request is acted on, but there is no
periodic expiry transition today. Until Identity adds an owner-side lifecycle
transition, the combined history view labels an overdue `pending` request as
“expired · owner transition pending” from its expiry timestamp. It does not
mutate the record or offer it as a live approval. Whether Identity should add a
scheduled expiry sweep, and what audit event it should emit, remains an
Identity-owned lifecycle decision.

## Route and API implications

Current route ownership remains:

```text
/                         homepage
/docs/                    docs
/login                    identity
/logout                   identity
/admin                    shared Platform admin shell
/admin/operations         Platform-owned read-only status and runbook links
/admin/identity           Identity-owned admin section
/admin/workshop           Registry-owned catalog inventory
/admin/artifacts          Artifact-owned release inventory
/admin/build-cache        Build Cache-owned target snapshot inventory
/api/admin/overview       root-authorized service health summary
/api/admin/operations     root-authorized latest deploy/restore status
/api/admin/deploy-requests Platform-owned deployment requests and stages
/api/admin/ssh-access/requests Platform-owned temporary SSH requests
/api/identity/v1/admin/permission-requests Identity-owned role permission requests
/api/identity/            identity API
/api/diagnostics/         diagnostics API
/api/build-cache/         current target snapshot API
```

Future routes should follow domain ownership rather than convenience:

- publish/pipeline: protected publish/promotion API, route name not locked yet;
- artifacts: package/blob API, route name not locked yet;
- registry/catalog: public metadata and protected write API, route name not
  locked yet;
- MCP/capability: no route until the cross-service auth contract is mature.

The `/admin` shared shell composes links and status only. Each protected data or
action route still authorizes at its owning service; the root does not copy
service state or policy.

## State implications

Current state ownership:

```text
/var/lib/vapor-server/homepage       mostly stateless today
/var/lib/vapor-server/docs           docs artifacts and pointers
/var/lib/vapor-server/identity       identity SQLite DB and sessions
/var/lib/vapor-server/diagnostics    diagnostics runs and indexes
/var/lib/vapor-server/registry       Registry catalog metadata
/var/lib/vapor-server/updates        immutable Artifact releases
/var/lib/vapor-server/operations     latest root deploy/restore status pointer
/etc/vapor-server                    server-local env/secrets; included only in the new, intentionally unencrypted recovery-bundle design
```

The deployed state-only bundle predates this recovery design and excludes the
configuration directory. Schema-v2 export code now includes it, but that source
change is not yet deployed or validated by a clean-host restore drill. See
`docs/operations-evidence-and-recovery-contract.md` for the current contract.

Future state should be service-owned:

- publish/pipeline: publish requests, promotion status, release/publish audit;
- artifacts: immutable blob/package state and manifests;
- registry/catalog: catalog metadata, visibility, channels/version pointers;
- capability surface: capability definitions/grants/audit only, not domain data.

Whole-system backup/restore remains composed by `Vapor-Platform-Server`, but each
stateful service should define its own export/import contract over time. No
service should read another service's database as an integration path.

## Cross-service auth contract

This contract should be defined before docs, diagnostics, publish, artifacts,
catalog, toolchain, or MCP depend on identity for privileged actions.

Implemented today:

- identity owns profiles, linked Steam/GitHub identities, roles, sessions, and
  identity audit;
- docs upload/export and diagnostics read/export still use temporary
  token/admin-token scaffolds;
- server-local admin tokens are bootstrap/emergency tools, not normal product
  or developer workflow authority.

Required future contract shape:

- Identity is the canonical authority for user/session/role decisions.
- Other services must consume identity-issued authorization or call an explicit
  identity authorization API. They must not copy roles, mint roles, or inspect
  the identity database directly.
- Operator-facing authority should identify people by external identities
  such as SteamID64 and GitHub login, not internal profile row IDs.
- Authorization results need enough information for downstream policy:
  subject, linked external identities, effective roles/capabilities, proof or
  session class, expiry, and audit correlation.
- Browser Steam OpenID proves browser control of a Steam account. It is not by
  itself enough for protected developer publishing or pipeline operations.
- Steamworks Web API tickets are the stronger Steam client/backend proof for
  future client-originated developer workflows.
- `root` implies `content-developer`, but Steam-side publishing still requires
  separate Steamworks/pipeline authority.
- Service-to-service or pipeline actors must be explicit principals with narrow
  authority. GitHub Actions should only request the VPS-owned deploy service,
  not receive server/product secrets.

Initial authority targets:

| Capability | Current authority | Future authority direction |
| --- | --- | --- |
| Homepage/docs public read | None | None |
| Docs upload/export | Docs token scaffold | Root or protected pipeline authority |
| Diagnostics upload | Explicit opt-in unauthenticated | Still opt-in; optional identity context only if policy requires it |
| Diagnostics list/download/export | Diagnostics admin-token scaffold | Root session or identity-issued root capability |
| Identity role grant/revoke/audit | Root session or server-local bootstrap token | Same, with server-local token reserved for emergency/bootstrap |
| Publish request | Not implemented | Steam profile + linked GitHub + `content-developer`/`root` + required Steamworks/pipeline authority |
| Artifact write | Not implemented | Publish/pipeline principal |
| Registry/catalog write | Not implemented | Publish/pipeline principal or authorized developer/root policy |
| Deployment/recovery | VPS-local operator paths and restricted GitHub trigger | Same narrow split; do not move secrets into GitHub Actions |
| MCP/capability actions | Not implemented | Identity-authenticated, least-privilege, audited capabilities |

## Risks

- Root orchestration absorbs service business logic.
- Identity becomes a generic backend instead of the authority service.
- Docs becomes generic artifact storage.
- Diagnostics drifts into telemetry or fingerprinting.
- Publishing authority is treated as only a Vapor role and loses the separate
  Steamworks/pipeline authority requirement.
- Shared status views copy service policy or become a bypass around service-owned authorization.
- MCP/capability work exposes broad remote authority before services have clear
  authorization boundaries.

## Do not build yet

- MCP/ACP integration.
- Server-mediated Steam publishing credentials as a normal user workflow.
- Public telemetry or Git-backed diagnostics transport.
- Generic artifact blob storage before publish/catalog contracts exist.
- A broad toolchain facade over unstable internal APIs.
- A full global admin dashboard/launcher GUI. **Superseded by the accepted
  shared Platform admin toolbench design dated 2026-11-02.** Implement only the
  service-owned sections and routes in that design; do not restore a standalone
  Identity `/admin` page or add a compatibility alias.
- Service-to-service database reads.

## Next concrete step

The shared authorization contract is documented in
`docs/cross-service-authorization-contract.md`. New service-owned admin routes
must use that contract; the Platform shell must remain an aggregator, and
future Build Cache administration waits for the Build Cache service itself.

## Callback pointer

Vapor server boundary callback: read repo docs first; treat docs as authority;
separate implemented, incomplete, near-term planned, and speculative; preserve
clean service/domain ownership and auth boundaries; no live VPS, secrets, SSH,
deploys, or mutations unless explicitly requested; do not design internals or
build premature services.
