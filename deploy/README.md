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
- `scripts/run-approved-deploy.sh`: atomically claim one approved, unexpired
  request, verify its immutable root tree and every submodule gitlink, check the
  commit remains reachable from the protected branch, verify the exact
  first-party submodule source URLs, then invoke only the fixed deployment
  script. A request is terminal after one execution attempt.
- `scripts/deploy-atlas.sh`: validate and atomically publish the Atlas commit
  pinned by the approved root revision under `/srv/vapor-web/atlas`, separately
  from Docs.
- `scripts/install-auto-deploy.sh`: install a systemd timer that polls the
  approved-request worker. An empty queue is a no-op; the timer never deploys
  the latest branch by itself.
- `scripts/install-state-backup.sh`: install a systemd timer that periodically
  runs `export-state.sh`.
- `scripts/harden-vps.sh`: make SSH key-only explicit and enable UFW for
  SSH/HTTP/HTTPS.
- `scripts/install-github-actions-deploy-user.sh`: legacy/manual service helper;
  GitHub Actions no longer receives SSH access or permission to start deploys.
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
- `scripts/export-state.sh`: create a root-only `.tar.gz` state bundle under
  `/var/backups/vapor-server` by default. The bundle includes
  `/var/lib/vapor-server` state and a manifest, not `/etc/vapor-server`
  secrets. `VAPOR_BACKUP_RETENTION_COUNT` controls retention for automatically
  named state bundles and defaults to 48.
- `scripts/restore-state.sh`: restore a bundle created by `export-state.sh`
  onto an already bootstrapped server; requires `--yes` and moves prior state
  aside before replacing it. The script writes a safe latest restore-result
  record after restoring state and restarting services.
- `scripts/install-systemd.sh`: install/enable the composed host unit.
- `scripts/install-services-systemd.sh`: install/enable the individual service
  units for `VAPOR_SERVER_MODE=services`.
- Platform's request database lives at
  `${VAPOR_STATE_ROOT}/platform/platform.sqlite3` and is included in whole-state
  export/import bundles.
- In independent-services mode, the cross-service `/admin` toolbench returns
  `503` instead of routing the removed Identity `/admin` page. The shared
  toolbench is served by the composed Platform process in the default mode.
- `systemd/vapor-artifact.service`: independently runnable immutable release
  delivery service for `VAPOR_SERVER_MODE=services`.
- `systemd/vapor-build-cache.service`: independently runnable target snapshot
  service for `VAPOR_SERVER_MODE=services`; state lives under
  `/var/lib/vapor-server/build-cache`.
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
