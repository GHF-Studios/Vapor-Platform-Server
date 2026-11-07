# Vapor platform workstream map

Use this page to find the current decisions before following an older design or
unchecked implementation plan. Older notes preserve design history; they do
not override the current architecture below.

## Current sources of truth

- [`2026-11-06-vapor-platform-operations-and-recovery-redesign.md`](specs/2026-11-06-vapor-platform-operations-and-recovery-redesign.md)
  defines the Platform, service ownership, admin, deployment approval, and
  whole-system recovery boundaries.
- [`2026-11-07-independent-service-deployments.md`](specs/2026-11-07-independent-service-deployments.md)
  defines full versus one-service deployment from a pinned Platform
  composition.
- [`2026-11-07-pin-deploy-topology.md`](plans/2026-11-07-pin-deploy-topology.md)
  records the approved topology in each deployment request and worker handoff.
- [`2026-11-07-platform-approvals-inbox.md`](plans/2026-11-07-platform-approvals-inbox.md)
  defines the owner-routed dashboard inbox; Platform is the shared interface,
  while Platform and Identity remain authoritative for their own requests.
- [`../deployment-status.md`](../deployment-status.md) records what was
  actually built, CI-validated, or installed. A design approval is not evidence
  of a live deployment.

## Cross-cutting constraints

- One VPS may host the platform, but its services run and deploy independently
  behind the shared public gateway. Platform composes full deployments and
  whole-system export/import; it does not absorb service-owned business rules.
- Deployments are human-approved requests pinned to an exact root revision,
  tree, target, and service submodule composition. No push-following or
  periodic deploy-latest timer is an authorization path.
- Service-owned behavior, schemas, and APIs belong in their service
  repositories. Keep those repositories checked out as root-level submodules.
- No secrets, game files, or private keys belong in source or recovery bundles.

## Historical documents

Documents dated November 1–3 may contain earlier single-process, timer-based,
or pre-approval assumptions. Read them for the problem context, but follow the
current sources above for implementation and deployment decisions. Their
historical observations should not be silently rewritten as if they were
current operational evidence.
