# Operations evidence and recovery contract

Status: source-level contract. Live restore exercises require separate explicit
authorization.

## Purpose

`Vapor-Platform-Server` owns composed operational recovery. Individual services own
their own state semantics over time, but root backup/restore remains the
whole-system recovery wrapper for the single-VPS slice.

## State bundle

`deploy/scripts/export-state.sh` writes a gzip tar bundle shaped as:

```text
vapor-server-state/
  manifest.toml
  submodules.txt
  state/
    diagnostics/
    docs/
    identity/
    operations/
    registry/
    updates/
    build-cache/
```

The bundle excludes `/etc/vapor-server` and records:

```toml
schema_version = 1
secrets_included = false
```

Backups are recovery bundles, not artifact distribution, not audit storage, and
not a replacement for service-owned export/import formats.

Build Cache state is included because it owns the current target snapshots and
candidate staging. Its service is stopped during export/restore so promotion
cannot race a state copy. Builder credentials and signing keys remain in
`/etc/vapor-server` and are never exported.

## Admin deployment/recovery status

The Platform Operations page reads a single root-owned deployment/recovery
status record at `${VAPOR_STATE_ROOT}/operations/latest.json` (configured as
`VAPOR_PLATFORM_OPERATIONS_FILE` for the Platform process). Deploy and restore
scripts write it via a same-directory temporary file and atomic rename. The
containing directory is `root:vapor` mode `0750`; the record is `root:vapor`
mode `0640` so the read-only Platform process can inspect it.

Schema version 1 contains only the operation (`deploy` or `restore`), result
(`success` or `failed`), start and finish UTC timestamps, checked-out server
revision (or `unknown`), and runtime mode (`monolith` or `services`). The
reader bounds the file to 4 KiB, rejects unknown fields and invalid values, and
returns no raw file content or filesystem location. The endpoint requires a
root Identity session and is private/no-store. The record is a current status
pointer, not an append-only audit log or proof that every service feature is
ready; detailed operator evidence remains in the deployment logs and runbooks.

Restore writes its result after restoring state and attempting to restart the
previously active services, so the restore result replaces any pre-restore
record that was included in the state bundle. It never reports the bundle path,
error output, environment contents, or credentials.

## Restore validation

`deploy/scripts/restore-state.sh` runs
`deploy/scripts/state-bundle-validate.py` before extraction, before stopping
services, and before replacing `/var/lib/vapor-server`.

The validator rejects:

- unreadable or non-gzip tar archives;
- absolute paths;
- parent-directory traversal;
- entries outside `vapor-server-state/`;
- symlinks and hardlinks;
- device nodes, FIFOs, sockets, and unsupported entry types;
- duplicate archive entries;
- bundles without `vapor-server-state/manifest.toml`;
- bundles without `vapor-server-state/state/`;
- unsupported manifest schema versions;
- manifests that do not assert `secrets_included = false`.

The validator does not print archive member contents.

## Restore behavior

Restore remains a root-only operation. It:

1. validates the bundle;
2. takes the deploy/state lock;
3. stops stateful services;
4. extracts into a temporary directory;
5. moves existing state aside under the backup root;
6. installs restored state under `/var/lib/vapor-server`;
7. reapplies expected ownership and restrictive permissions;
8. restarts previously active services;
9. runs the local health check.

This is intentionally not a remote operations API.

## Future work

- Exercise restore on a disposable VPS.
- Add service-owned semantic export/import formats.
- Add restore rehearsal evidence to deployment status only after an actual
  verification run.
- Keep secrets in `/etc/vapor-server`; do not add them to state bundles.
