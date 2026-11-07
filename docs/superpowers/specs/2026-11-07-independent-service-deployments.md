# Independent Service Deployment Design

## Goal

Allow an operator to approve and deploy one service from a known Platform
composition without rebuilding or restarting neighboring services. Full
Platform deployments remain available for changes to Platform, gateway routing,
systemd, shared configuration, or topology.

## Current State

- Platform requests approve one root commit, root tree, runtime mode, and the
  complete set of root-level submodule pins.
- The approved-deploy worker verifies the complete composition, checks out the
  approved root commit and every pinned service commit, then calls the full
  `deploy.sh` path.
- In services mode, that path builds seven service binaries plus Platform,
  installs the shared units and Caddy routes, and restarts all services.
- Each service already has its own package, systemd unit, loopback port, state
  directory, health endpoint, admin route ownership, and Caddy path.
- The shared gateway and service processes are implemented; selective deploy
  and per-service rollback are not.

## Design

1. Keep Platform as the only approval and orchestration authority. Reuse the
   existing authenticated deployment request and owner approval flow; add no
   second identity or authorization system.
2. Keep the Platform root commit as the exact composition record. It pins the
   source commit for every root-level service submodule. A scoped request names
   one allowlisted deployment unit in that composition. A service-source
   change is deployable only after the Platform root pin points to it.
3. Model request scope explicitly as `full` or `service`. `full` requires no
   service target. `service` requires one of `atlas`, `homepage`, `docs`,
   `identity`, `diagnostics`, `registry`, `artifact`, or `build-cache`;
   `platform` is always a full deployment because it owns the approval API and
   shared gateway. Atlas is a target because it has its own pinned source and
   atomically published static release tree; it does not require restarting a
   service process.
4. Full push-triggered requests preserve current behavior. A main-only manual
   workflow dispatch may request one service target. CI still validates the
   exact root revision before submitting either request; approval is still
   required before the VPS worker acts.
5. The worker verifies the same root commit, tree, repository and all submodule
   pins for either scope. Full scope calls the existing deployment path.
   Service scope is allowed only in `services` runtime mode and calls a fixed
   service deployment map, never a request-supplied executable, unit, path, or
   port. The worker must not move the canonical deployment checkout to the
   request revision before a scoped deployment. It creates a temporary
   isolated local clone at the verified root commit, initializes only the
   selected target submodule, builds from that clone, and removes it after
   completion. The private clone keeps its own `.git/modules` directory; Git
   documents submodule support in linked worktrees as incomplete.
   Existing full deployments may retain their current checkout behavior.
6. The service deployer builds only the selected package. Before changing the
   service, it requires the expected unit to be loaded and healthy, stages the
   new binary, and stops only that unit. It snapshots that service's mutable
   state while preserving its ownership and mode metadata. Artifact and Build
   Cache storage are explicit exceptions: their release archives and target
   snapshots are immutable content outputs, so a binary-only deployment
   preserves these stores in place rather than copying potentially large
   artifacts. Build Cache target output is also excluded from recovery exports
   as reproducible. Its staged upload directory may contain incomplete
   candidates, but publication changes only after an atomic pointer update. No
   artifact/build-cache state schema migration is performed by deployment.
   It atomically swaps the binary, restarts the service, and checks its fixed
   loopback health endpoint plus the exact public path and status contract
   defined for that target (respecting Caddy prefix stripping and routes that
   intentionally keep the prefix). Do not invent a shared `/healthz` route:
   it is loopback-only and not generally exposed through the gateway. Failure
   restores the previous binary and service state, then starts only the
   selected unit. The deployed-source manifest is committed atomically as part
   of success; if that write fails, rollback the target too. An incomplete
   rollback preserves its backup and is reported as incomplete.
7. Record the deployed root composition and service source commit per service
   in a bounded, atomic, non-secret manifest under the existing
   `VAPOR_STATE_ROOT/operations` tree. Full deployment refreshes all entries;
   scoped deployment changes only its target after health succeeds. The
   schema-v2 exporter already copies the whole state tree, so this manifest is
   carried by existing whole-platform export/import; validate it when present
   without adding a new archive category or pretending an old bundle contains
   a record it lacks.
8. No Caddy, systemd unit definition, environment file, or shared topology is
   modified by scoped deployment. Those require a full request.

## Non-goals

- Deploying arbitrary commits, branches, repositories, binaries, commands, or
  paths.
- Updating a service directly from an unpinned submodule commit.
- Allowing Platform itself to update through the service-only operation.
- Simultaneously executing conflicting or overlapping deployments. Only one
  active request per root commit is allowed; submitting another scope for a
  commit with an active request returns a clear conflict instead of returning
  the unrelated active request.
- Claiming independent deployability on the live VPS before a scoped request is
  approved, executed, and observed in the service version manifest.

## Acceptance

- A full request retains the existing full deployment behavior.
- A service request identifies scope and target in the approval dashboard and
  is rejected unless its source map is exact and runtime mode is `services`.
- A successful scoped request builds, swaps, restarts, and health-checks only
  its selected service; no neighbor unit or Caddy configuration changes.
- Invalid target, monolith mode, stale source pin, inactive/missing unit,
  failed build, and failed health check cannot be reported as success.
- Injected post-swap failure restores the previous binary and state; failed
  restoration preserves recovery material and is clearly reported.
- Scoped binary replacement preserves the `root:<service-group>` executable
  ownership expected by systemd, and state rollback restores prior service
  ownership. Artifact and Build Cache payload data are never copied as part of
  code deploys.
- Atlas can be deployed and rolled back by its fixed target without restarting
  a neighbor service.
- The deployed-source manifest changes atomically only after health success
  and is included automatically in the existing state export/import. Missing
  records in older exports remain unknown.
