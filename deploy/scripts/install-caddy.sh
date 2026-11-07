#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "${SCRIPT_DIR}/lib.sh"

require_root

CADDY_TEMPLATE="${VAPOR_CADDY_TEMPLATE:-${DEPLOY_DIR}/caddy/Caddyfile.template}"

if ! command -v caddy >/dev/null 2>&1; then
  echo "error: caddy is not installed; run bootstrap-ubuntu.sh first" >&2
  exit 1
fi

fallback_block="$(mktemp)"
candidate_caddyfile="$(mktemp /etc/caddy/Caddyfile.XXXXXX)"
trap 'rm -f "${fallback_block}" "${candidate_caddyfile}"' EXIT

if [ -n "${VAPOR_HTTP_FALLBACK_HOST}" ]; then
  cat > "${fallback_block}" <<EOF
http://${VAPOR_HTTP_FALLBACK_HOST} {
	redir https://${VAPOR_DOMAIN}{uri} permanent
}
EOF
fi

sed "s/{{VAPOR_DOMAIN}}/${VAPOR_DOMAIN}/g" \
  "${CADDY_TEMPLATE}" \
  | sed \
    -e "/{{VAPOR_HTTP_FALLBACK_BLOCK}}/r ${fallback_block}" \
    -e "/{{VAPOR_HTTP_FALLBACK_BLOCK}}/d" \
  > "${candidate_caddyfile}"

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
if [ -n "${VAPOR_HTTP_FALLBACK_HOST}" ]; then
  echo "caddy: installed canonical HTTP redirect for ${VAPOR_HTTP_FALLBACK_HOST}"
fi
