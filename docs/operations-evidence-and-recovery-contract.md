# Operations evidence and recovery contract

Status: legacy schema-v2 implementation record. For the current target for new
backups, see `docs/superpowers/specs/2026-11-08-state-only-owner-recovery.md`.
The hourly full-host/plaintext-secret design below is superseded and must not be
used as the specification for new archives. Existing v2 archives remain
untouched until a state-only copy and restore have been validated.

## Purpose

`Vapor-Platform-Server` owns composed operational recovery. Individual services
own their state semantics; Platform coordinates a consistent archive of the
primary VPS state, its runtime configuration, and the source pins needed to
rebuild the host. The recovery copy lives on an owner-held USB drive and is
intentionally unencrypted.

## Recovery layers

- **Strato account:** controls the virtual server and its reinstall/recovery
  controls. If the guest OS or its root access is lost, recover this account
  first and provision a clean supported Ubuntu host.
- **Root SSH key:** direct break-glass access to a running guest OS, independent
  of Vapor Identity. Keep its private key separate from the USB archive.
- **USB recovery set:** hourly service-state/configuration generations and
  exact source pins. It is intentionally plaintext, contains credentials, and
  is storage rather than a security key or hardware module.
- **Steam/GitHub:** remain their own account and authorization boundaries.
  Restoring Vapor cannot restore access revoked by either provider; recovery is
  not complete until configured integrations pass their readiness checks.
- **Optional second VPS:** may add build capacity and another backup copy once
  its capacity is known. It is replaceable and must not become the only backup
  or a prerequisite for local builds, deployment, or primary-host operation.

These layers address different failures. The USB cannot replace Strato account
access, a root SSH key cannot restore lost service state, and a second node
cannot replace either owner-controlled recovery path.

## State bundle

`deploy/scripts/export-state.sh` writes an hourly gzip tar bundle shaped as:

```text
vapor-platform-recovery/
  manifest.toml
  source/
    root-commit.txt
    submodules.txt
    toolchain.txt
  state/
    diagnostics/
    docs/
    identity/
    operations/
    registry/
    updates/
    build-cache/                 # target snapshots/staging excluded as rebuildable
  config/
    vapor-server/                # includes service credentials
  host/
    caddy/Caddyfile
    systemd/vapor-*.service
    systemd/vapor-*.timer
```

The bundle includes `/etc/vapor-server` and records:

```toml
schema_version = 2
secrets_included = true
archive_encryption = "none"
```

If `VAPOR_ADMIN_DOMAIN` is configured, the recovery archive preserves that
hostname and the matching Caddy/Identity settings. A fresh-host activation then
depends on the admin DNS record and the provider callback configuration being
ready for that hostname (including the exact GitHub OAuth callback, if enabled).
Do not bring the restored dashboard online on the public hostname while its
Identity canonical origin points at the isolated admin host; complete the DNS,
HTTPS, and login checks first.

The manifest lists included categories, runtime mode, source and recursive
submodule pins, the exporting host's detected OS/architecture, stable Rust/Cargo
versions and compiler host triple observed when the export ran, creation time,
and rebuildable exclusions. These compiler values describe the export host;
they do not prove which compiler produced the binaries currently serving
requests. Fresh-host reconstruction must select a compiler/toolchain from the
source/build contract and record that choice as part of the rebuild operation.
The manifest contains no secret values. The VPS publishes an archive index with each
retained generation's name, byte size, timestamp, and SHA-256, plus a separate
latest-generation pointer. The USB pull uses the index and verifies each
download against its declared size and digest. Bundles are recovery material,
not release distribution or audit storage.

Build Cache's service-owned metadata is included, while target snapshots and
temporary candidate staging are excluded as reproducible build output. All
active state-writing services are quiesced during archive copying, including
Registry and Artifact. Platform is stopped before its temporary SSH expiry
timers; the exporter stops those timers and waits for already-queued expiry
helpers to finish before copying SQLite state. It resumes the services and
previously active timers after the state copy, and aborts rather than publish a
snapshot if systemd state cannot be inspected or drained. Credentials/signing
keys in the config directory are
part of the intentionally unencrypted recovery archive; logs, manifests,
dashboard responses, and Git never print their values.

The VPS retains complete generations for USB catch-up. A Windows scheduled task
uses the owner's existing root SSH key to fetch unseen generations when the
128 GB drive is mounted. It verifies SHA-256 before promoting a local copy and
does not delete a previous good copy if transfer or validation fails. A later
second VPS may hold a replica, but neither it nor the USB is required for
ordinary service operation.

The export writes a new index that excludes planned pruning candidates, then
publishes the new latest pointer, and only then deletes those candidates. The
planner protects both the previous indexed latest archive and the incoming
generation. If the existing index/pointer cannot be reconciled with the
archive and checksum files, pruning is skipped; an extra generation is safer
than deleting an uncertain recovery point.

## Admin deployment/recovery status

The Platform Operations page reads the current deployment/recovery status
record at `${VAPOR_STATE_ROOT}/operations/latest.json` (configured as
`VAPOR_PLATFORM_OPERATIONS_FILE` for the Platform process). Deploy and restore
scripts write it via a same-directory temporary file and atomic rename. The
containing directory is `root:vapor` mode `0750`; the record is `root:vapor`
mode `0640` so the read-only Platform process can inspect it. The History view
also reads append-only deploy/restore outcomes from the Platform-owned
`platform_operation_history` table in `${VAPOR_STATE_ROOT}/platform/platform.sqlite3`.
The deploy/restore scripts invoke the append helper as the Platform service
account so SQLite and WAL files remain writable by the running Platform process.
Each outcome has a unique operation ID and stable cursor key; history is not a
cross-service request ledger.

Schema version 1 contains only the operation (`deploy` or `restore`), result
(`success` or `failed`), start and finish UTC timestamps, checked-out server
revision (or `unknown`), and the fixed runtime topology (`services`). The
reader bounds the file to 4 KiB, rejects unknown fields and invalid values, and
returns no raw file content or filesystem location. Both endpoints require a
root Identity session and are private/no-store. The latest record remains a
current status pointer; operation history is a durable outcome list, not an
append-only audit log of every operator action or proof that every service
feature is ready. Detailed operator evidence remains in deployment logs and
runbooks.

Independent deployments also maintain
`${VAPOR_STATE_ROOT}/operations/deployed-sources.json`. Schema version 1 maps
each deployed target to the exact Platform composition commit, pinned target
commit, and deployment timestamp. Full-stack deploys replace the complete
inventory; an approved single-service deploy updates only its target after
health checks. The file is non-secret, written atomically as `root:vapor`
mode `0640`, and included through the existing state-tree export. Recovery
validation checks its schema when present; older bundles without it remain
valid and do not receive invented deployment records.

Restore writes a `success` or `failed` status after its final recovery action,
so the restore result replaces any pre-restore record that was included in the
state bundle. On failure, stderr/systemd journal output distinguishes a
completed rollback from an incomplete rollback; the compact status record does
not encode that distinction yet. It never reports the bundle path, error output,
environment contents, or credentials.

## Restore validation

`deploy/scripts/restore-state.sh` checks the detached SHA-256 and runs
`deploy/scripts/state-bundle-validate.py` before extraction or stopping
services.

Validation also caps archives at 100,000 entries, 2 GiB per regular file, and
8 GiB total expanded regular-file data. These limits are checked from tar
headers before restore extracts any member, preventing a valid-looking archive
from exhausting the staging filesystem through a large member set or compressed
expansion.

Before staging or stopping anything, restore also refuses symlinked/masked
Vapor units, non-regular managed unit entries, and a symlinked Caddyfile path;
rollback does not silently erase those host policies. Pre-restore copies use a
unique temporary suffix so two restores in the same second cannot reuse a
backup location.

The validator rejects:

- unreadable or non-gzip tar archives;
- absolute paths;
- parent-directory traversal;
- entries outside `vapor-platform-recovery/`;
- symlinks and hardlinks;
- device nodes, FIFOs, sockets, and unsupported entry types;
- duplicate archive entries;
- bundles without `vapor-platform-recovery/manifest.toml`;
- bundles without state, config, and source-pin categories;
- unsupported manifest schema versions;
- manifests that do not explicitly declare plaintext secrets included.

The validator does not print archive member contents.

## Restore behavior

Restore remains a root-only operation. It:

1. validates the bundle;
2. checks that the clean Platform checkout and all submodule pins exactly match
   the bundle, and assumes service binaries have already been built from those
   revisions, before mutation;
3. takes the deploy/state lock and extracts into a temporary directory;
4. stops stateful services, then stops and drains temporary SSH expiry work and
   the state-export timer before replacing SQLite state;
5. stages and backs up existing state and configuration;
6. installs state, `/etc/vapor-server`, Caddy config, and recorded Vapor units;
7. reapplies ownership/modes, reloads systemd/Caddy, restores active-grant expiry
   timers where applicable, and runs local readiness;
8. attempts to roll state/configuration back before restarting services if a
   step fails, and stops short if it cannot stop the old services safely;
9. records restore success/failure and writes the rollback outcome to
   stderr/systemd journal output.

This is intentionally not a remote operations API.

## Fresh-host coordinator

`deploy/scripts/recover-fresh-host.sh` composes bootstrap, a non-activating
runtime build/install, and transactional state restore for a newly provisioned
Ubuntu x86_64 host. It checks the bundle checksum and schema, requires the same
Ubuntu major/minor release and host architecture recorded in the bundle,
checks out the exact root commit and recursive submodule pins, installs the
recorded Rust toolchain, and builds with the committed lockfiles. During this
stage, package service starts are suppressed, service/Caddy units are installed
but not enabled, and the state-export timer is not started. It then restores
state/configuration; `restore-state.sh` starts the restored target units only
after all files have been installed and validated. This avoids exposing a
fresh default or partially restored Vapor stack before the recovery snapshot
is in place.

The coordinator refuses non-empty Vapor checkout, state, or config paths and
an existing Caddyfile on a new run. It creates a root-owned marker bound to the
bundle digest and source revision; an interrupted run may be resumed only with
`--resume` and the same bundle. Resume rechecks the source checkout and replays
staging and restore. It never cleans unrelated paths.

Temporary SSH accounts, authorized keys, and sudoers fragments are host-local
and are not copied into a recovery archive. Same-host restore stops and drains
expiry work before replacing state, then re-arms expiry timers for active,
interrupted, or failed grants whose account cleanup may be incomplete. A failed
grant whose compensating cleanup completed is not re-armed; missing or unsafe
timer units for any grant that may remain usable fail the restore instead of
leaving access without automatic cleanup. A fresh-host restore marks previously active,
provisioning, or failed temporary grants as revoked with a `system_recovery`
audit event, clears their stored public keys and granted scopes, and omits their
expiry units. Pending requests remain available for a new owner decision. The
fresh host therefore requires new SSH requests and approvals; it never presents
an untransferred grant as active access.

New recovery exports carry a source-pinned recovery kit inside the archive:
recover-fresh-host.sh, recovery_bundle_metadata.py, state-bundle-validate.py,
deployed_source_manifest.py, service_deploy_map.py, a short operator README,
and SHA-256 entries for those files. The source-manifest validator imports the
closed service map, so both files must travel with the standalone validator.
The fresh-host entrypoint checks kit consistency and runs the
validator before apt can change host packages, then validates again from the
checked-out source revision before changing application state. Preflight needs
the base Ubuntu image to supply Python 3.11+ and stops without package changes
if that runtime is absent. Kit/archive checksums detect corruption but are not
signatures: the operator must run a kit from the trusted, owner-held recovery
copy. An archive sidecar must contain one digest entry naming exactly the
selected archive. The Windows USB sync extracts the kit
from the latest content-verified archive into Recovery-Kit/<root-commit>/ and
atomically updates Recovery-Kit/latest.json only after checking the embedded
hashes. Its sync report states whether the latest archive supplied a usable
kit and whether it supports pre-package validation. The reported
`retained_archive_payload_bytes` counts only recovery archive payload files,
not sidecars, extracted kits, or other USB contents. Complete previous kits
remain usable with their matching archive but do not claim the newer preflight
guarantee. Generations without the kit remain copyable but do not produce a
ready kit pointer. The owner can move the matching kit folder, archive, and
sidecar to a fresh host without independently finding or downloading bootstrap
files. The coordinator then checks out the exact recorded source/submodules
and revalidates against that source. A clean-host exercise remains necessary
to verify this chain end to end.

The final machine check requires Identity to report Steam identity and the
dashboard as configured and ready. GitHub browser login is required only when
either GitHub OAuth credential is configured; when neither is configured,
GitHub remains optional. These booleans verify server configuration, not
successful end-to-end provider credentials. The recovery marker remains until
the owner completes configured provider checks in a browser, verifies the owner
dashboard, and reruns with
`--resume --confirm-provider-checks`. That flag is an explicit owner attestation;
the script does not claim to independently prove third-party OAuth credentials.
Exit status `3` means restoration is complete and waiting for that owner check;
it is distinct from a failed rebuild.
Failures after deployment disable and stop the newly installed Vapor/Caddy
units, preventing them from restarting on reboot, and retain the marker for review. Until a clean disposable-host run and those browser
checks are completed, the path remains unproven.

The recovery archive is intentionally plaintext and contains credentials by
owner decision. Its SHA-256 sidecar detects corruption only when the archive
and sidecar arrive through a trusted path; it is not a signature or proof of
source authenticity. The operator must trust the physical backup and the
recorded source pin. Owner-held signing/authenticity verification remains later
hardening work.

## Copy and recovery boundary

- `deploy/scripts/backup-to-usb.ps1` is the local pull path. It needs an explicit
  SSH identity path and USB destination; it does not assume a drive letter.
  `deploy/scripts/install-usb-backup-task.ps1` registers an hourly current-user
  task plus a logon catch-up trigger; it does not install an SSH key or register
  itself during source deployment.
  It reads the explicit latest-generation pointer and re-hashes every indexed
  archive on every sync. A damaged indexed copy is restored from the VPS and
  verified before it replaces the local file. Unindexed/manual files are
  preserved and reported as unverified. Capacity pruning is allowed only after
  every indexed archive has passed its content check. Unknown or damaged files
  count against capacity and are left untouched.
  A per-destination OS mutex serializes the scheduled task with manual catch-up
  before stale partial files are removed; the Windows fixture test covers
  initial copy, catch-up, pointer mismatch, hash failure, and overlapping runs.
  The VPS index and sidecars provide corruption checks, not authenticity
  against a compromised VPS.
- The root SSH private key is held separately from the USB backup. It is the
  ordinary break-glass path if Identity is unavailable.
- Strato account recovery/reinstall is the out-of-band path if the host/root
  key is unavailable. Preserve evidence and rotate provider/SSH credentials
  before restoring a host suspected of compromise.
- Do not claim a clean-host restore or USB catch-up is proven until each has
  actually been exercised on a disposable host and the owner's drive.

## Independent admin gateway current status (verified 2026-11-07)

Platform commit `12192d85348432ee24d1ce7c02ff2dab6eeded84` is deployed in
services mode. Caddy sends `/admin*` and `/api/admin*` to Platform; Platform
serves its own routes and proxies fixed service-admin prefixes to loopback
owner services. Owner APIs and action authorization remain service-owned. The
shared shell uses the explicit
`application/vnd.vapor.admin-page.v1+html; charset=utf-8` fragment contract;
the earlier marker-based whole-page wrapper described in the original baseline
is no longer current. The deployed `deploy/scripts/health-check.sh` passed
service, fixed admin-route, unauthenticated-contract, Atlas-through-Caddy, and
state-export-timer checks on the live host. See
[`deployment-status.md`](deployment-status.md) for the latest request queue,
backup generation, and public release-feed state.

New source-level recovery exports record the Rust and Cargo versions plus the
compiler host triple observed at export time in `source/toolchain.txt` and the
manifest. They are diagnostic host metadata, not compiler provenance for the
running binaries. The export does not install a compiler or build binaries.
