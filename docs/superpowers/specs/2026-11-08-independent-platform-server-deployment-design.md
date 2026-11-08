# Independent Platform Server Deployment

## Intent

Platform Server is both the shared admin gateway and one runtime service. It
currently deploys only as part of the full service composition, while the other
services and Atlas have scoped deployment targets. Give Platform Server its own
approved, rollback-capable `platform` target so gateway/application changes do
not restart unrelated services.

## Goals

- Treat the root `vapor_platform_server` binary as a first-class scoped service.
- Build and replace only that binary, with service stop/start, health checks,
  Platform-state snapshot, source-manifest update, and rollback on failure.
- Verify that the detached source tree is the exact approved root commit and
  tree; because Platform is the root package, do not require a root gitlink.
- Automatically classify known Platform-server Rust sources to `platform`.
- Keep operations-helper source, root build/deploy configuration, submodule
  composition changes, and unknown paths on the conservative full-deployment
  path.
- Keep approval, request ordering, audit records, and dispatch in the existing
  Platform deployment workflow.

## Non-goals

- No new service repository or submodule for Platform Server.
- No independent deployment of the root-only `vapor_platform_ops` helper.
- No changes to full-composition deployment behavior, service ownership, or
  trust boundaries.
- No approval-order bypass or direct database mutation.

## Constraints

- `service_deploy_map.py` is the fixed target allowlist consumed by the root
  deployer; only its allowlisted values reach file, unit, or binary operations.
- Platform Server runs as `vapor-platform-server.service`, listens on loopback
  port 7110, and exposes `/healthz` through the shared gateway.
- Its SQLite and operation state lives at `/var/lib/vapor-server/platform` and
  must be snapshotted while the service is stopped so rollback can restore it.
- Only the explicit Platform-server source allowlist maps to `platform`:
  `src/main.rs`, `src/admin.rs`, `src/admin_auth.rs`, `src/admin_gateway.rs`,
  `src/deploy_request_schema.rs`, `src/deploy_requests.rs`,
  `src/emergency_console.rs`, `src/operation_history.rs`, `src/pagination.rs`,
  `src/platform_ops_client.rs`, `src/recovery.rs`, `src/ssh_access.rs`,
  `src/ssh_key.rs`, and `src/ssh_schema.rs`. `src/recovery_status.rs` and
  `src/ssh_policy.rs` are shared with the root-only ops binary; ops-helper and
  unknown Rust source changes require full deploy.
- `Cargo.toml`, `Cargo.lock`, deployment scripts/configuration, submodule pins,
  or unknown root files continue to select full composition.
- The current pending docs-only request must be rejected through the normal
  operator UI before a later request can be approved; implementation must not
  bypass that queue rule.

## Approaches

1. **Keep Platform full-only.** Lowest implementation cost, but violates the
   independent-service deployment goal and restarts all services for gateway
   changes.
2. **Add an allowlisted root `platform` target (selected).** Reuse the existing
   scoped worker and rollback lifecycle, with a root-source verification path.
   This is the smallest change that gives the gateway independent deployment.
3. **Split Platform Server into another repository/submodule.** Creates a new
   repository and migration boundary without changing the fact that the root
   deployer owns the gateway and its platform state; unnecessary for the
   current one-binary service.

## Design

The target map gains a `platform` service record with the root package path,
`vapor_platform_server` binary, `vapor-platform-server.service`, loopback
health at `http://127.0.0.1:7110/healthz`, public health at `/healthz`, and
state name `platform` with snapshot rollback. The deploy helper recognizes this
record as a root service: it verifies the detached root commit/tree and clean
checkout, builds only the named binary, snapshots Platform state, replaces the
binary atomically, restarts only the Platform unit, validates loopback and
public health, updates the deployed-source manifest, and restores the prior
binary/state on failure.

The isolated build checkout still initializes and verifies every pinned
service submodule for this target because the root Cargo package declares
those crates as path dependencies. That checkout is build input only: the
approved target continues to mutate only the Platform binary, state snapshot,
and Platform systemd unit.

The target is added consistently to the GitHub request allowlist, Platform API
validation, root queue helper, and Admin target metadata. Workflow dispatch
offers `platform`. Push classification maps only the explicit Platform source
allowlist to `platform`; shared ops-helper modules and all other root
runtime/configuration paths continue to resolve to full. A commit touching
Platform source plus a service submodule remains full composition.

## Verification

- Classifier tests prove Platform-only source changes select `platform`, ops
  helper and unknown root changes select `full`, and mixed Platform/service
  changes select `full`.
- Deploy-helper tests prove root-source verification skips gitlink lookup only
  for the `platform` target and still checks exact root commit/tree and clean
  status.
- Queue/API/admin tests prove `platform` is accepted as a service target and
  malformed scope/target combinations remain rejected.
- Scoped deploy tests prove only `vapor-platform-server.service` is stopped,
  replaced, health-checked, and restarted; failure restores the prior binary,
  state, and source manifest.
- Full composed CI runs on Linux and Windows. No live deployment occurs as part
  of this implementation until an operator approves the generated request.
