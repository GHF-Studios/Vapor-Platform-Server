# Owner-operated server workbench

## Intent

Give the owner one browser workbench for the small set of Vapor servers they
operate. It should explain each machine's purpose, show resource and service
health, expose safe configuration, and offer typed operations whose intent and
outcome survive a rebuild or migration. The owner must be able to deliberately
wipe one host, move selected Vapor responsibilities to the other, validate
them, and later move them back.

## Goals

- Treat the core VPS and optional worker/backup host as two deliberate,
  operator-managed machines, not elastic capacity.
- Show cached, timestamped telemetry and service state without confusing host
  metrics with service health.
- Let the owner run explicit host actions such as restarting one named Vapor
  service or rebooting a selected server.
- Record each action's actor, target, typed parameters, configuration/revision
  context, lifecycle, and result in backed-up Platform state.
- Make safe non-secret configuration inspectable and editable through typed
  forms, with a preview and durable version record before applying.
- Support an explicit, resumable rebuild/migration runbook between the two
  hosts, with checkpoints for export, profile application, service movement,
  verification, and retirement.
- Preserve a local build/maintenance path if the optional worker is absent.
- Keep Strato as the authority for hardware allocation, VNC, and recovery-OS
  operations.

## Non-goals

- Hyperscaling, metric-triggered lifecycle changes, automatic role-based job
  placement, or unattended host migration/retirement.
- Calling Strato APIs to create, resize, or delete VPS hardware.
- Arbitrary SSH, shell, VNC, file browsing, or command-string entry as the
  normal control surface.
- Treating server labels or telemetry credentials as authorization.
- Replacing Identity, Steam, GitHub, Strato, or owner-held recovery with a new
  authority service.
- Moving service-specific business logic or service state into Platform.

## Approaches

1. **Dashboard SSH to each host.** Rejected: it makes the web process a holder
   of broad private keys and turns a UI action into an opaque remote command.
2. **One root-running auto-management agent.** Rejected: it couples telemetry,
   scheduling, and privileged control and suggests unattended fleet behavior.
3. **Owner-triggered typed operations and runbooks through a narrow per-host
   executor** (selected): Platform authenticates the operator through the
   existing root Identity session, records each typed step, and dispatches only
   a fixed action vocabulary. The owner reviews each checkpoint and chooses to
   continue, retry, or abort. No arbitrary command text is accepted. The
   workflow can rebuild/move responsibilities between the two hosts but does
   not acquire or delete VPS hardware; those remain Strato operations.

The analogy is only about the convenience of a single remote operations
surface. It does not imply remote screen or keyboard sharing: normal use is
telemetry, typed controls, previews, audit history, and explicit workflow
checkpoints. Any future emergency terminal remains a separate high-risk tool.

## Current evidence

- `src/server_fleet.rs` samples the core VPS locally and returns cached host
  metrics. `src/node_fleet.rs` stores manually added remote-node metadata and
  accepts bounded telemetry from `vapor-node-agent`.
- Node-agent credentials only authorize telemetry. Current purpose values are
  `control-plane`, `builder`, `backup`, and `general`; no scheduler uses them.
- `src/platform_ops_client.rs` sends bounded JSON to a root-owned local socket.
  The helper handles deployments, temporary SSH grants, recovery snapshots and
  status, and emergency-console lifecycle.
- `deploy/scripts/vapor-ssh-service-control` has an exact service allowlist,
  but is granted only to temporary SSH accounts, not the browser.
- `platform_operation_history` covers deploy and restore only.
- Server Fleet shows metrics and registration/revocation, but no typed per-host
  operation or configuration UI.

## Design boundaries

### Inventory and telemetry

- Inventory records describe hosts the owner provisioned. Adding/removing a
  record never creates/destroys hardware. Decommissioning a record is an
  explicit, auditable Vapor action that revokes its agent credential and
  preserves its history; the owner removes/reinstalls VPS hardware through
  Strato.
- Purpose communicates what each server is for. It may select a profile in an
  explicit operator action but does not itself schedule jobs or alter a node.
- Telemetry remains a separate reporting capability and cannot authorize an
  operation.
- The core VPS remains visible from local sampling even if all remote agents
  are offline.

### Operations

- Every action selects a named host and enum-backed operation. Initial actions
  are `restart_service(service_id)`, `reboot_host`, and lifecycle steps such as
  `prepare_rebuild`, `apply_profile`, `move_workload`, `verify_host`, and
  `decommission_record`. The UI states impact and requires explicit
  confirmation for interruption/destruction.
- The browser accepts no shell, path, unit-name, executable, or argument text.
- Platform records the requested operation and actor before dispatch. A
  compare-and-set state machine records accepted, running, succeeded, failed,
  and outcome-unknown states with timestamps and bounded result details.
- Only an explicitly retried step may run again. An operation with uncertain
  outcome is reconciled by the owner; there are no blind retries.
- The core VPS uses the existing local root-operation socket and Identity root
  session check. A remote host receives a separately installed, bounded typed
  executor; its telemetry credential remains telemetry-only.
- A migration is an owner-driven, resumable workflow with a durable run ID,
  source/target host IDs, selected responsibilities, pinned profile/state
  revisions, per-step result, and explicit resume/abort actions. A lost HTTP
  response cannot be treated as proof that a migration completed.
- Workload movement uses the owning service's export/import/deploy contract.
  Platform coordinates and records the steps; it does not read or copy service
  databases directly.
- Raw emergency shell remains a separately gated break-glass path, never the
  ordinary server workbench.

### Configuration and recovery

- The browser shows only an allowlisted, typed, non-secret configuration
  profile. Secrets remain in root-owned host configuration and owner-held
  recovery; secret values are not echoed to the browser or operation log.
- A proposed edit creates a versioned profile revision and preview. Applying
  it is an explicit typed operation tied to that revision; history records the
  revision ID and a redacted diff.
- Setup/recovery reapplies the selected profile to a replacement host. Strato
  owns allocation, network setup, VNC, and recovery-OS actions; the Vapor
  wizard makes the handoff and return-to-Vapor steps explicit.
- Inventory, operations, profiles, and migration checkpoints must be included
  in Platform's state-only export/import contract before this is migration-
  ready.

## Rollout

1. Correct Server Fleet copy and make the no-autoscaling/manual-action rule
   explicit.
2. Add the Platform operation ledger/API and core-VPS typed actions.
3. Install a bounded executor on remote hosts and show per-host capabilities.
4. Add typed profile read/edit/preview/apply for safe non-secret settings.
5. Add an explicitly advanced, resumable rebuild/move runbook using the actual
   service-owned export/import/deploy APIs.
6. Verify export/import of inventory, operations, profile revisions, and
   migration checkpoints; perform the wipe/move/return drill across both hosts.

## Acceptance

- The UI states that actions are owner-triggered and there is no automatic
  scaling or metric-triggered lifecycle behavior.
- The core host remains visible when remote telemetry agents are unavailable.
- Each supported host action accepts only its enum payload and records actor,
  target, action, context, and outcome before reporting success.
- Unknown/failed execution never appears successful or automatically replays.
- Config values are typed and allowlisted; secrets cannot be read or logged.
- A state backup includes operation, profile, and migration records and
  restores them on a clean platform installation.
- No remote host appears controllable until its typed executor is installed,
  authenticated, and reports its supported operation set.
- The two-host drill can move selected responsibilities to the survivor, wipe
  and rebuild the first host through its provider, then move responsibilities
  back without losing the operation/configuration record.
