# Deployment

`deploy/` contains the reproducible server deployment material for the current
single-VPS vertical slice.

Current baseline:

- Ubuntu 26.04 LTS;
- no Plesk;
- Caddy reverse proxy and HTTPS;
- systemd service units;
- SQLite-backed identity state;
- human-approved, exact-revision deployment from `Vapor-Platform-Server`;
- state under `/var/lib/vapor-server`;
- service env files under `/etc/vapor-server`;
- source checkout under `/opt/vapor-platform-server`.
- composed host process by default, with independent service processes retained
  through the explicit `VAPOR_SERVER_MODE=services` option.

No secrets belong in this repository. The bootstrap script creates server-local
admin tokens in `/etc/vapor-server/*.env` when those files do not already exist.

## First-run outline

After DNS points `vapor.ghf-studios.site` at the VPS, run the explicit
first-install bootstrap once. Normal updates then require a CI request and a
root approval in the Operations dashboard:

```bash
sudo deploy/scripts/bootstrap-ubuntu.sh
sudo deploy/scripts/deploy.sh --bootstrap
```

The scripts are intended to be idempotent, but they are still real server
operations: package installation, system user creation, systemd unit
installation, Caddy configuration, initial source checkout, service build, and
service restart. `--bootstrap` is accepted only when the deployment checkout is
empty. It is an explicit first-install/migration action; updates use approvals.

Before DNS is active, a temporary HTTP-only fallback can be rendered by setting
`VAPOR_HTTP_FALLBACK_HOST` when installing Caddy or running `deploy.sh`. This is
for smoke testing only; the intended public endpoint remains
`https://vapor.ghf-studios.site/` after DNS and certificate issuance work.

## Scripts

- `scripts/bootstrap-ubuntu.sh`: install OS packages, create users/directories,
  and create server-local env files.
- `scripts/deploy.sh`: perform the explicit first bootstrap or deploy one exact
  revision after verifying the worker's SQLite claim. It publishes Atlas and
  builds/restarts either the composed host (default) or independently runnable
  services (`VAPOR_SERVER_MODE=services`). The composed mode requires the root
  `Cargo.lock`. It writes a safe latest-result record under
  `${VAPOR_STATE_ROOT}/operations/`.
- `scripts/deploy-service.py`: deploy one approved, allowlisted target in
  services mode. The request pins the complete Platform composition; the
  worker builds only the selected package from an isolated temporary clone,
  initializes only that target submodule, and leaves the canonical checkout
  and its submodule metadata unchanged. Daemon targets replace one
  `root:vapor` executable, stop/start only its unit, and run fixed health
  checks. Atlas swaps its static `current` release link and checks the public
  route without restarting daemons. Failed updates restore the selected target
  and its manifest; incomplete rollback preserves its recovery copy. Small
  mutable service state is snapshotted with ownership and modes retained.
  Artifact releases and Build Cache target archives are immutable, so a
  binary-only update preserves those payload trees in place rather than
  copying potentially multi-gigabyte outputs.
- `scripts/service_deploy_map.py`: closed target map for packages, executables,
  units, state policies, and public/loopback health routes. Target values come
  from the approved request; paths and operations come only from this map.
- `scripts/deployed_source_manifest.py`: atomically records each live target's
  Platform composition and pinned service revision under
  `${VAPOR_STATE_ROOT}/operations/deployed-sources.json`; full deployments
  refresh the complete map and scoped deployments update only their target.
- `scripts/run-approved-deploy.sh`: atomically claim one approved, unexpired
  request, verify its immutable root tree and every submodule gitlink, check the
  commit remains reachable from the protected branch, verify the exact
  first-party submodule source URLs, then invoke only the fixed deployment
  script. A request is terminal after one execution attempt.
- `scripts/deploy-atlas.sh`: validate and atomically publish the Atlas commit
  pinned by the approved root revision under `/srv/vapor-web/atlas`, separately
  from Docs.
- `scripts/install-approved-operations.sh`: install request-specific deploy
  units and remove the retired periodic deploy timer.
- `scripts/install-platform-ops.sh`: install the fixed root operation helper,
  SSH wrappers, and the isolated temporary-account SSH policy.
- `scripts/report-cutover-preflight.sh`: upload a short-lived, secret-free
  report of the active VPS source revision, relevant service states, path
  metadata, and expected route status codes. It does not change host state.
- `scripts/remove-platform-ssh-access.sh`: disable new temporary SSH grants and
  remove only this feature's SSH policy after every grant is revoked or expired;
  the shared deploy-operation helper stays installed.
- `scripts/install-auto-deploy.sh`: retained as a guard that rejects the
  retired timer-based path.
- `scripts/install-state-backup.sh`: install a systemd timer that periodically
  runs `export-state.sh`.
- `scripts/harden-vps.sh`: make SSH key-only explicit and enable UFW for
  SSH/HTTP/HTTPS.
- `scripts/configure-identity-auth.sh`: update server-local identity provider
  configuration in `/etc/vapor-server/identity.env` without committing secrets,
  including GitHub browser OAuth credentials and public redirect origin.
- `scripts/configure-authorization.sh`: create or reconcile separate
  server-local Identity authorization tokens for Platform, Registry, Artifact,
  and Build Cache, plus a separate Build Cache orchestrator token. Values are
  written only to protected env files and never printed.
- `scripts/grant-identity-role.sh`: grant `root` or `content-developer` to an
  already linked Steam+GitHub developer/root profile by requiring both SteamID64
  and GitHub login, using the server-local identity admin token for bootstrap
  or emergency operator use.
- `scripts/export-state.sh`: create a root-only full-platform recovery archive
  under `/var/backups/vapor-server`. The target schema includes service state,
  `/etc/vapor-server` credentials/configuration, selected host routing files,
  and exact source/submodule pins. The archive is intentionally unencrypted;
  build-cache target output is excluded as reproducible. An atomic latest
  pointer is published only after validation and SHA-256 calculation.
- `scripts/backup-to-usb.ps1`: pull unseen completed archives to an explicitly
  selected USB directory over the owner's existing SSH key. The script
  verifies SHA-256 before retaining a copy, rechecks the latest generation on
  every run, rotates a recheck through older copies, and can catch up after
  missed runs. A per-destination Windows mutex prevents a scheduled run and a
  manual catch-up from deleting or replacing each other's in-progress files.
  `-MaxStoredBytes` defaults to 100 GiB and bounds archive payload bytes; it
  accepts 1 MiB through 120 GiB. The current archive is never pruned, and
  unverified/manual archives are preserved; if they leave no room for the
  current archive, sync fails before deleting verified history. The report's
  `retained_archive_payload_bytes` excludes sidecars, extracted kits, and other
  USB contents.
  `deploy/tests/test_backup_to_usb.ps1` exercises catch-up, pointer rejection,
  corrupt downloads, byte-budget pruning/refusal, and overlapping runs using
  local fixtures. VPS checksums detect corruption, not a compromised VPS
  replacing both the archive and index.
  For new archives, it also verifies and extracts the source-pinned recovery
  kit into `Recovery-Kit/<root-commit>/`, then atomically writes
  `Recovery-Kit/latest.json`. Its report marks when an older archive has no
  embedded kit. The kit contains the exact fresh-host entrypoint, its bootstrap
  metadata reader, matching checksums, and concise run instructions.
- `scripts/install-usb-backup-task.ps1`: register an hourly current-user
  Task Scheduler job plus a logon catch-up trigger once the USB and SSH key
  paths are known. The job runs while that Windows account is signed in;
  missed hourly generations are fetched from the VPS index when the drive is
  mounted.
- Example from a Windows checkout (use the actual drive letter and existing
  root SSH identity):

  ```powershell
  .\deploy\scripts\install-usb-backup-task.ps1 -Server vapor.ghf-studios.site -SshKey "$HOME\.ssh\id_ed25519" -UsbPath 'E:\Vapor-Recovery'
  ```

  This copies the current sync script to `%LOCALAPPDATA%\Vapor\Recovery` and
  registers a per-user task; it does not create or copy a private key. Rerun the
  installer after updating the checkout to refresh that copy. The first run
  requires a trusted OpenSSH `known_hosts` entry for the VPS and an SSH identity
  available without an interactive passphrase prompt. Run `backup-to-usb.ps1`
  directly with the same arguments for immediate or manual catch-up; the task
  runs hourly while the account is signed in.
- `scripts/restore-state.sh`: restore a full-platform bundle onto a bootstrapped
  server with the exact clean Platform and submodule revisions recorded in the
  bundle and the service binaries already built from them; requires `--yes`,
  stages prior state/configuration, and attempts rollback before restarting
  services if restore fails. It rejects managed host symlinks it cannot safely
  roll back and uses a unique pre-restore directory. It does not bootstrap a
  fresh OS or build binaries.
  A clean-host drill is still required before calling this a proven recovery
  path.
- `scripts/recover-fresh-host.sh`: guarded fresh-host coordinator for supported
  Ubuntu x86_64 hosts. It checks the owner-held kit and validates the USB
  bundle before package changes, then checks it again against the exact
  source/submodule pins. The base host must supply Python 3.11+; otherwise
  preflight stops without package changes. Internal checksums detect
  corruption but do not authenticate a replaced kit; run only the kit from
  your trusted recovery copy. It checks that the Vapor
  install/state/config paths are empty, installs the recorded Rust/Cargo
  toolchain, builds and stages the pinned runtime with
  package/service activation held back, restores the bundle, then activates
  restored units and checks configured Steam/GitHub/dashboard readiness.
  Run as root with `--bundle PATH --yes`. It refuses existing Vapor
  data/configuration on a new run; if interrupted after it creates its recovery
  marker, rerun with `--resume` and the same bundle. It requires the bundle to
  contain a current exact toolchain observation and validates the archive with
  the validator from its pinned source commit. After restoring, it leaves the
  site running but keeps a recovery marker until the owner signs in with Steam
  and GitHub and verifies the owner dashboard; then rerun with
  `--resume --confirm-provider-checks`. Exit code `3` means the restore is
  complete and waiting for those owner checks. The Windows sync places the
  matching entrypoint, metadata reader, archive validator, and validator
  dependency in `Recovery-Kit/<root-commit>/`, alongside its README and file
  checksums; use that kit folder and archive together. Older complete kits
  remain usable but report that they lack the newer pre-package validation
  step. A failed post-restore recovery
  disables and stops the newly deployed Vapor services, the export timer, and
  Caddy, and preserves its marker for review. This
  source path has not yet been exercised on a clean VPS; a successful local
  test is not a recovery drill.

The Windows sync script refuses destinations not reported as removable unless
`-AllowFixedDrive` is supplied after confirming the USB volume. It verifies the
SSH host through the existing OpenSSH known-host record, downloads into a
temporary directory, checks byte count and SHA-256, then renames the bundle into
the recovery folder. The USB archive is intentionally plaintext and contains
server credentials; keep the drive physically controlled. The sync follows the
explicit latest-generation pointer and checks it against the archive index; it
does not infer recency from a timestamp/hash filename. It rehashes the latest
copy and one rotating historical copy each run. Before deleting any
generation for capacity, it rehashes every indexed candidate and removes only
the oldest candidate whose payload digest matches the current VPS index.
Unknown, stale, or damaged files count against capacity and are left untouched;
if they prevent safe pruning, the sync stops. The report distinguishes copies
hashed this run from archive files merely retained on the drive.
- `scripts/install-systemd.sh`: install/enable the composed host unit.
- `scripts/install-services-systemd.sh`: install/enable the individual service
  units and Platform admin gateway for `VAPOR_SERVER_MODE=services`.
- Platform's request database lives at
  `${VAPOR_STATE_ROOT}/platform/platform.sqlite3` and is included in recovery
  bundles.
- In independent-services mode, Platform runs as the shared admin gateway on
  port 7110. It serves its own pages and forwards only fixed service-owned
  admin page prefixes; each owner service still checks the session/capability
  and owns mutations/audit. The services-mode deployment builds and installs
  this gateway alongside the independently deployed services. The gateway
  source change is not live until an approved deployment installs it.
- `systemd/vapor-artifact.service`: independently runnable immutable release
  delivery service for `VAPOR_SERVER_MODE=services`.
- `systemd/vapor-build-cache.service`: independently runnable target snapshot
  service for `VAPOR_SERVER_MODE=services`; state lives under
  `/var/lib/vapor-server/build-cache`.
- `docs/vapor-build-snapshot-contract.md`: contract for one signed rolling
  compiler snapshot per locked target. An optional second VPS may build
  candidates; the local path remains available when no worker exists. This
  server currently composes and administers Build Cache but does not yet create
  snapshots.
- `scripts/install-caddy.sh`: render/install the Caddy config.
- `caddy/Caddyfile.services.template`: path routing for the independent
  service-process mode; the default template routes through the composed host.
- `scripts/health-check.sh`: check local service health endpoints.
- `scripts/public-http-check.sh`: check public HTTP routes before DNS/HTTPS is
  ready.
- `scripts/smoke-identity-auth.sh`: exercise the identity auth-attempt flow,
  including GitHub Device Flow and optional Steam ticket/root bootstrap.
- `scripts/smoke-docs-upload.sh`: upload a placeholder docs page through the
  token-protected docs endpoint.
- `scripts/smoke-diagnostics.sh`: upload a diagnostics smoke run and verify
  obvious secret redaction.
- `scripts/build-vapor-client-docs-bundle.sh`: build a curated Vapor docs tar.gz
  bundle from a local Vapor Client checkout.
- `scripts/upload-docs-via-http.sh`: upload a docs bundle to the public docs
  route, such as `http://82.165.77.104/docs` before DNS is ready.
- `scripts/upload-docs-via-ssh.sh`: stages a public docs bundle over SCP, then
  prompts for VPS sudo access and posts from the VPS to its local docs service.
  The docs admin token stays in the server's root-readable configuration.
- `scripts/deploy-vapor-client-docs.sh`: build and deploy the curated docs bundle
  in one operator command.

## Temporary SSH operator workflow

The installed `/usr/local/libexec/vapor-platform-ops` helper exposes a closed, root-only local
operator interface. It never prints the stored public key and requires a real
terminal for every mutation:

```text
sudo /usr/local/libexec/vapor-platform-ops ssh list
sudo /usr/local/libexec/vapor-platform-ops ssh show <request-id>
sudo /usr/local/libexec/vapor-platform-ops ssh verify <request-id>
sudo /usr/local/libexec/vapor-platform-ops ssh grant <request-id> --capability vapor_deploy --capability vapor_service_control --duration 8h
sudo /usr/local/libexec/vapor-platform-ops ssh revoke <request-id>
sudo /usr/local/libexec/vapor-platform-ops ssh reconfigure <request-id>
```

Grant prints the fingerprint, purpose, requested and selected scope, and expiry;
the operator must type the fingerprint exactly. `host_admin` is exclusive and
requires a second `UNRESTRICTED ROOT` confirmation. It provisions a request-
specific account with unrestricted sudo; connect as the displayed temporary
account and use `sudo -i` for a root shell. It does not grant `root@host` SSH.
Revoke requires typing
`YES`. Reconfigure explains the revoke-and-new-request process and makes no
change. `verify` is read-only and compares an active grant with its managed
account, authorized key, sudo policy, and expiry timer; it never prints key
material. Local mutations appear as `local-root` in the SSH audit trail; web
approvals remain attributed to the Identity profile.
