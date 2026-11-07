# Vapor Platform Server

`Vapor-Platform-Server` is the deployment and operations repository for Vapor's public
web/server surface. It intentionally lives beside Vapor Client; it is not a
submodule of the game/app root.

## Intended service topology

```text
Vapor-Platform-Server/
  Vapor-Homepage-Server/      -> Vapor-Homepage-Server
  Vapor-Docs-Server/          -> Vapor-Docs-Server
  Vapor-Identity-Server/      -> Vapor-Identity-Server
  Vapor-Diagnostics-Server/   -> Vapor-Diagnostics-Server
  Vapor-Registry-Server/      -> Vapor-Registry-Server
  Vapor-Artifact-Server/      -> Vapor-Artifact-Server
  Vapor-Build-Cache-Server/   -> Vapor-Build-Cache-Server
  Atlas/                      -> Atlas

  deploy/
    caddy/
    systemd/
    scripts/

  docs/
    decisions-and-backlog.md

  crates/
    vapor_server_shared/   # optional later
```

Production runs Homepage, Docs, Identity, Diagnostics, Registry, Artifact,
Build Cache, and the Platform admin gateway as independent service processes
behind Caddy. Platform owns deployment coordination, shared operator navigation,
and whole-system recovery; each service retains its own routes, state, and
business logic. Services can be rebuilt and deployed independently, while the
Platform recovery workflow composes the whole host. The single-process
`monolith` mode remains available as an alternate deployment topology, not the
production topology.

## Single-domain route map

```text
/                         -> homepage
/atlas/                   -> Atlas static knowledge site
/docs/                    -> docs
/login                    -> identity browser login/register
/admin                    -> shared Platform admin toolbench
/admin/approvals          -> pending Platform and Identity decisions
/admin/deployments        -> Platform-owned deployment lifecycle and stages
/admin/ssh-access         -> Platform temporary access and active grants
/admin/hosting            -> service health and feature readiness
/admin/history            -> bounded owner-sourced request and operation history
/admin/operations         -> Platform operations pages via view query
/admin/workshop           -> Registry-owned read-only catalog inventory
/admin/identity           -> Identity-owned admin section
/admin/artifacts          -> Artifact-owned read-only release inventory
/admin/build-cache        -> Build Cache-owned read-only target snapshot inventory
/api/admin/overview       -> root-authorized service health summary
/api/admin/operations     -> root-authorized latest deploy/restore status
/api/identity/            -> identity
/api/diagnostics/         -> diagnostics
/api/registry/            -> registry
/api/updates/v1/vapor/    -> Artifact service: signed Vapor application releases
/api/build-cache/         -> Build Cache service: signed target snapshots
```

## State model

- Homepage: expected to remain mostly stateless.
- Docs: owns uploaded/generated docs artifacts and current/version pointers.
- Atlas: owns its source in `GHF-Studios/Atlas`; Platform Server builds that
  source into a separate static tree served at `/atlas/`. It has no mutable
  server-side state.
- Identity: owns linked Steam/GitHub identities, roles, and auth/session state.
- Diagnostics: owns opt-in diagnostics uploads, indexes, retention metadata,
  and export/import data.
- Registry: owns the canonical Vapor content catalog and Workshop inventory.
- Operations: Platform owns the latest deploy/restore status pointer; deployment
  and recovery actions remain CLI-owned.
- Artifact: serves immutable Vapor application release manifests, signatures,
  and archive bytes from its own release store. It runs as an independent
  service in production; release build/sign/publish tooling remains an operator
  workflow.
- Build Cache: owns one current signed Cargo snapshot per supported target and
  candidate promotion state. VPS-hosted target VM orchestration and Vapor Client
  installation are not yet implemented; an empty cache is not a usable build
  distribution.

## Restore model

The long-term goal is reproducible rebuild from source plus either:

- explicit empty initialization; or
- import from a previously exported server state bundle.

Each stateful service should eventually support its own export/import. The Platform Server
repo composes those into whole-conglomerate export/import.

## Current status

The repository contains the single-VPS, independent-service deployment slice:
Caddy, separate systemd service units, CI-created human-approved deployment
requests, SQLite-backed identity state, health checks, smoke checks, and
whole-server file-state export/restore scripts. Current live deployment and
recovery evidence is tracked in `docs/deployment-status.md`; source changes
alone do not imply a VPS deployment.

The service implementations remain owned by their own repositories. Atlas is
separately owned source built as static output. Platform does not move service
handlers or persistence code into this repository.

See `docs/decisions-and-backlog.md` for the accepted boundaries, pending
decisions, and future/backburner items that should not be lost.

See `docs/vertical-slice-contracts.md` for the current cross-service contracts:
routes, auth, state ownership, deployment, smoke checks, and known weak seams.

See `docs/cross-service-authorization-contract.md` for the target identity
introspection contract that should replace service-local token scaffolds.

See `docs/docs-publication-contract.md` for current documentation publication,
validation, and export semantics.

See `docs/atlas-publication-contract.md` for the separate Atlas static-site
publication and retention contract.

See `docs/diagnostics-report-contract.md` for the structured diagnostics
upload, privacy, redaction, storage, and quota contract.

See `docs/operations-evidence-and-recovery-contract.md` for state-bundle
validation, restore behavior, and recovery boundaries.

See `docs/human-approved-operations-api.md` for the CI request, root approval,
exact-revision worker, and one-time bootstrap contract.

See `docs/deployment-status.md` for the latest recorded VPS deployment status.

See `docs/identity-auth-runbook.md` for Steam/GitHub identity provider
configuration and auth-flow smoke testing.

See `docs/steam-authority-model.md` for the distinction between Steam browser
identity, Steamworks/WebAPI verification, Vapor roles, and publishing authority.

See `docs/vapor-app-release-api.md` for the public signed application update
feed, and `docs/vapor-standalone-install-and-source-home-contract.md` for the
standalone installer, local game mounting, and Vapor-managed source checkout
lifecycle.

The Platform toolbench shell contains no operator data. Its overview API checks
the Identity root session on loopback, then probes only service health routes;
raw probe bodies and environment configuration are never returned. Feature
readiness stays `not reported` until each service exposes a safe readiness DTO.
