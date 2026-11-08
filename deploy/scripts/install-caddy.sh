#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "${SCRIPT_DIR}/lib.sh"

require_root
validate_vapor_host_config

CADDY_TEMPLATE="${VAPOR_CADDY_TEMPLATE:-${DEPLOY_DIR}/caddy/Caddyfile.template}"

if ! command -v caddy >/dev/null 2>&1; then
  echo "error: caddy is not installed; run bootstrap-ubuntu.sh first" >&2
  exit 1
fi

candidate_caddyfile="$(mktemp /etc/caddy/Caddyfile.XXXXXX)"
trap 'rm -f "${candidate_caddyfile}"' EXIT

render_args=(
  --template "${CADDY_TEMPLATE}"
  --public-domain "${VAPOR_DOMAIN}"
)
if [ -n "${VAPOR_ADMIN_DOMAIN:-}" ]; then
  render_args+=(--admin-domain "${VAPOR_ADMIN_DOMAIN}")
fi
if [ -n "${VAPOR_HTTP_FALLBACK_HOST}" ]; then
  render_args+=(--http-fallback-host "${VAPOR_HTTP_FALLBACK_HOST}")
fi
python3 "${DEPLOY_DIR}/scripts/render-caddyfile.py" "${render_args[@]}" > "${candidate_caddyfile}"

caddy fmt --overwrite "${candidate_caddyfile}"
caddy validate --config "${candidate_caddyfile}"

# Redirection into an existing Caddyfile preserves its previous mode. Ensure
# the unprivileged Caddy service can read the replacement configuration.
chown root:caddy "${candidate_caddyfile}"
chmod 0640 "${candidate_caddyfile}"
mv -f -- "${candidate_caddyfile}" /etc/caddy/Caddyfile
if [ "${VAPOR_RECOVERY_STAGING:-0}" != "1" ]; then
  systemctl enable caddy.service
fi

echo "caddy: installed config for ${VAPOR_DOMAIN}"
if [ -n "${VAPOR_ADMIN_DOMAIN:-}" ]; then
  echo "caddy: privileged dashboard routes isolated on ${VAPOR_ADMIN_DOMAIN}"
fi
if [ -n "${VAPOR_HTTP_FALLBACK_HOST}" ]; then
  echo "caddy: installed canonical HTTP redirect for ${VAPOR_HTTP_FALLBACK_HOST}"
fi
