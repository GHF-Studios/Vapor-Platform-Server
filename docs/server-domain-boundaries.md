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

This is an accepted target boundary, not a claim that the Registry API or its
persistent catalog is implemented today. Until then, client-side Workshop
tags are discovery hints only and must not be presented as enforced Vapor
semantics.

## Boundary map

| Domain | Boundary | Responsibilities | Non-responsibilities |
| --- | --- | --- | --- |
| Homepage | Implemented service | Public `/`, product/legal/home pages, public health | Login, docs, diagnostics, admin, publishing |
| Docs | Implemented service with temporary auth scaffolds | `/docs/`, docs artifacts, current/version pointers, docs export/import | Generic artifact storage, identity decisions, catalog |
| Identity | Implemented service and canonical auth owner | Steam profiles, GitHub links, roles, sessions, root/developer authorization, identity audit | Publishing execution, deployment, diagnostics/docs storage |
| Diagnostics | Implemented scaffold, policy incomplete | Opt-in upload, redaction, storage, indexes, retention, root-dev list/download/export | Telemetry, Git transport, hostnames, persistent machine IDs, identity authority |
| Publish/pipeline | Future real boundary | Protected publish requests, validation/promotion orchestration, publishing audit, controlled Steam/Workshop or server-publish authority | Identity source of truth, artifact blob semantics, public catalog reads |
| Artifacts | Future real boundary | Immutable packages/build blobs, checksums, manifests, retention/download policy hooks | Catalog metadata, publish approval, docs bundles |
| Registry/catalog | Future real boundary | Public metadata, listings, version/channel pointers, visibility/discovery | Blob storage, builds, identity roles, deploy orchestration |
| Toolchain/API UX | Future integration boundary | Stable CLI/Shell/launcher workflows over existing APIs | Canonical state, role decisions, publish execution |
| MCP/capability surface | Future capability boundary | Machine/action capabilities, least-privilege grants, capability audit | Business state, bypassing service APIs |
| Operations/recovery | Implemented root-owned workflow plus service contracts | Backups, restore/import composition, deploy health, emergency operator paths | Normal product workflows, service business decisions |
| Cross-domain admin UI | Future UI boundary | Aggregate operator views/actions once multiple services consume identity auth | Replacing domain services or storing canonical domain state |

## Route and API implications

Current route ownership remains:

```text
/                         homepage
/docs/                    docs
/login                    identity
/logout                   identity
/admin                    identity admin shell for now
/api/identity/            identity API
/api/diagnostics/         diagnostics API
```

Future routes should follow domain ownership rather than convenience:

- publish/pipeline: protected publish/promotion API, route name not locked yet;
- artifacts: package/blob API, route name not locked yet;
- registry/catalog: public metadata and protected write API, route name not
  locked yet;
- MCP/capability: no route until the cross-service auth contract is mature.

The current `/admin` route should not silently become the global admin console.
If admin operations span multiple domains, introduce an explicit cross-domain
admin boundary later.

## State implications

Current state ownership:

```text
/var/lib/vapor-server/homepage       mostly stateless today
/var/lib/vapor-server/docs           docs artifacts and pointers
/var/lib/vapor-server/identity       identity SQLite DB and sessions
/var/lib/vapor-server/diagnostics    diagnostics runs and indexes
/etc/vapor-server                    server-local env/secrets, not exported
```

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
- A global admin UI is built before the service auth contract exists.
- MCP/capability work exposes broad remote authority before services have clear
  authorization boundaries.

## Do not build yet

- MCP/ACP integration.
- Server-mediated Steam publishing credentials as a normal user workflow.
- Public telemetry or Git-backed diagnostics transport.
- Generic artifact blob storage before publish/catalog contracts exist.
- A broad toolchain facade over unstable internal APIs.
- A full global admin dashboard/launcher GUI.
- Service-to-service database reads.

## Next concrete step

Define the cross-service auth contract as the next design artifact. Keep it
abstract enough to avoid implementation internals, but precise enough that docs,
diagnostics, publish/pipeline, artifacts, registry/catalog, toolchain, and
future MCP/capability work can consume the same authority model.

## Callback pointer

Vapor server boundary callback: read repo docs first; treat docs as authority;
separate implemented, incomplete, near-term planned, and speculative; preserve
clean service/domain ownership and auth boundaries; no live VPS, secrets, SSH,
deploys, or mutations unless explicitly requested; do not design internals or
build premature services.
