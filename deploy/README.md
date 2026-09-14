# Deployment

`deploy/` contains the reproducible server deployment material for the current
single-VPS vertical slice.

Current baseline:

- Ubuntu 26.04 LTS;
- no Plesk;
- Caddy reverse proxy and HTTPS;
- systemd service units;
- SQLite-backed identity state;
- branch-based source deployment from `Vapor-Platform-Server`;
- state under `/var/lib/vapor-server`;
- service env files under `/etc/vapor-server`;
- source checkout under `/opt/vapor-platform-server`.

No secrets belong in this repository. The bootstrap script creates server-local
admin tokens in `/etc/vapor-server/*.env` when those files do not already exist.

## First-run outline

After DNS points `vapor.ghf-studios.site` at the VPS:

```bash
sudo deploy/scripts/bootstrap-ubuntu.sh
sudo deploy/scripts/deploy.sh
```

The scripts are intended to be idempotent, but they are still real server
operations: package installation, system user creation, systemd unit
installation, Caddy configuration, source checkout/update, service build, and
service restart.

Before DNS is active, a temporary HTTP-only fallback can be rendered by setting
`VAPOR_HTTP_FALLBACK_HOST` when installing Caddy or running `deploy.sh`. This is
for smoke testing only; the intended public endpoint remains
`https://vapor.ghf-studios.site/` after DNS and certificate issuance work.

## Scripts

- `scripts/bootstrap-ubuntu.sh`: install OS packages, create users/directories,
  and create server-local env files.
- `scripts/deploy.sh`: clone/update the Platform Server repo, update submodules, build
  services, install units/proxy config, and restart services.
- `scripts/install-auto-deploy.sh`: install a systemd timer that periodically
  runs `deploy.sh` against the configured branch.
- `scripts/install-state-backup.sh`: install a systemd timer that periodically
  runs `export-state.sh`.
- `scripts/harden-vps.sh`: make SSH key-only explicit and enable UFW for
  SSH/HTTP/HTTPS.
- `scripts/install-github-actions-deploy-user.sh`: create a restricted deploy
  user for GitHub Actions to trigger `vapor-deploy.service`.
- `scripts/configure-identity-auth.sh`: update server-local identity provider
  configuration in `/etc/vapor-server/identity.env` without committing secrets,
  including GitHub browser OAuth credentials and public redirect origin.
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
  aside before replacing it.
- `scripts/install-systemd.sh`: install/enable the four service units.
- `scripts/install-caddy.sh`: render/install the Caddy config.
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
- `scripts/upload-docs-via-ssh.sh`: alternate upload path that posts from the
  VPS to the local docs service.
- `scripts/deploy-vapor-client-docs.sh`: build and deploy the curated docs bundle
  in one operator command.
