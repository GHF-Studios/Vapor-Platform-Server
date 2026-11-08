# State-only owner recovery

**Status:** Current owner direction; replaces the full-host/plaintext-secret archive design for new backups.

## Intent

Make recovery understandable and repeatable without copying the operating system, service installation, source checkout, or secret configuration into every backup. Vapor setup owns rebuilding the host and re-entering provider configuration. A recovery archive preserves the mutable product data that cannot simply be recreated by installing the pinned software again.

## Owner decisions

- Keep frequent scheduled snapshots while pre-alpha state is changing quickly.
- Make backup status and deliberate snapshot/restore actions visible in the high-security Recovery area.
- New archives contain durable product state only. Do not include `/etc/vapor-server`, Caddy/systemd files, source checkouts, toolchains, recovery-kit scripts, or private configuration values. Schema 4 records only the Platform commit identifier needed to rebuild the source; it does not include source pins or source contents.
- Keep the owner USB copy unencrypted and physically controlled. This is backup storage, not an authentication key or hardware security module.
- Preserve currently existing archives until a new-format export and restore have both been validated. Do not silently delete the only existing off-host recovery copies during migration.
- Rebuild host packages, services, routing, and binaries from the current Platform setup/install flow. Re-enter credentials through that flow; provider credentials are not restored from the state archive.

## State classification

The archive is based on the service-owned persistent state root, with explicit inclusion/exclusion rules rather than copying the VPS filesystem.

- Keep service databases and durable service-owned content required to reconstruct the same Vapor instance.
- Keep publication data only when it is authoritative, mutable state and cannot be recovered from the official release workflow.
- Exclude build-cache snapshots, temporary candidates, process/runtime files, generated logs, transient diagnostics, host configuration, and rebuildable application artifacts.
- Each service must document its state paths and restore behavior. A new persistent path is not silently included merely because it is below a broad directory.
- The archive manifest may contain schema version, creation time, the Platform source revision identifier, category names, and file sizes/checksums. It must not contain credential values. It is not a source or host backup.

### Current VPS inventory (read-only inspection, 2026-11-08)

- `identity/identity.sqlite3`, `platform/platform.sqlite3`, and
  `registry/registry.sqlite3` are service-owned authoritative databases. Use
  SQLite's online backup API for each database, which captures committed WAL
  state without stopping the service or copying journal files. The three
  database snapshots are individually consistent; they are not a distributed
  transaction across services, so their capture instants can differ slightly.
- `operations/deployed-sources.json` is an operational inventory. Preserve it;
  regenerate `operations/latest.json` after restore so the system does not
  claim a pre-restore operation just occurred.
- `docs/current` and `docs/releases` contain published documentation content;
  those files are not reproduced by reinstalling the service binary and must
  remain recoverable unless an authoritative upstream is established.
- Artifact stores release payloads under `updates/releases/<version>/<platform>`.
  The current root is empty, but the release tree is included as published
  product data and never treated as host configuration or credentials.
- `diagnostics/runs` contains submitted diagnostic material. Its inclusion
  must follow the service's retention/privacy contract; until that contract is
  explicit, omit raw runs and preserve only non-sensitive service-owned
  metadata needed to operate the feature.
- `build-cache` is reconstructible cache state and is omitted in full.
- `homepage` is empty on the inspected host and has no durable user data today.

## Owner workflow

1. Recovery page shows current automatic snapshot cadence, retention, last successful snapshot, size, validation, and last owner USB copy report in a compact summary.
2. “Create snapshot now” produces and validates one state-only generation; “Download to USB” is an explicit owner action that transfers that generation to the browser for saving to the removable drive.
3. Restore is a separate, high-consequence action. It requires a verified state-only archive, a configured host, and an explicit confirmation naming the target instance. It replaces service state transactionally and preserves the pre-restore state until health checks finish.
4. Fresh-host recovery first runs the supported Vapor server setup flow, including package/dependency installation and secret re-entry, then imports the state archive. It does not try to recreate OS configuration from the archive.

## Compatibility and migration

- Legacy state exports use schema version 3. New exports use schema version 4, which adds an exact Platform source revision identifier, with a `vapor-platform-state-*` filename and isolated `state-only/` index and pointer. Schema 3 remains readable when its deployed-source inventory supplies one consistent Platform revision. Neither state format can overwrite or age out the full-host archive generations.
- Hourly state exports must not stop application services. Each SQLite database is snapshotted online and validated before publication.
- The currently retained full-host archives are legacy evidence. Keep them read-only during the transition; deployment disables the old timer so no more secret-bearing full-host archives are created.
- The restore path must state which format it is handling and must never infer host configuration from a state-only archive.
- The owner USB sync writes state snapshots to `State-Snapshots/` and a matching source-pinned recovery launcher to `Recovery-Kit/state-only/<commit>/`. It does not prune the legacy generations.

## Alternatives considered

1. Continue backing up `/etc`, host units, source pins, and secrets every hour. Rejected: it repeats setup/configuration in every generation and makes the USB a plaintext credential store.
2. Keep automatic state snapshots, but expose their health and deliberate export/restore actions in Recovery. Selected: protects rapidly changing pre-alpha data while separating rebuildable installation from durable state.
3. Manual snapshots only. Rejected for now: it creates avoidable data-loss gaps during active pre-alpha work.

## Acceptance

- A new archive has no config, host, source checkout, toolchain, recovery-kit, or credential entries.
- A schema-4 archive binds the exact Platform source commit used by the matching recovery kit without embedding code or host configuration.
- The manifest and validator distinguish state-only schema from any retained legacy archive.
- A clean Ubuntu x86_64 host can be rebuilt from the pinned source, prompt for provider credentials without echoing them, and import the archive without restoring host configuration from the archive.
- Failed import restores the prior state and leaves the pre-restore copy available.
- The Recovery page exposes compact status and explicit snapshot/download/restore actions with visible consequences and no generic shell access.
- The Windows USB flow verifies the new format and does not prune old generations during migration.
