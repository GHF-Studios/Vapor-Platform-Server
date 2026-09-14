#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "${SCRIPT_DIR}/lib.sh"

require_root

if ! command -v caddy >/dev/null 2>&1; then
  echo "error: caddy is not installed; run bootstrap-ubuntu.sh first" >&2
  exit 1
fi

fallback_block="$(mktemp)"
trap 'rm -f "${fallback_block}"' EXIT

if [ -n "${VAPOR_HTTP_FALLBACK_HOST}" ]; then
  cat > "${fallback_block}" <<EOF
http://${VAPOR_HTTP_FALLBACK_HOST} {
	redir https://${VAPOR_DOMAIN}{uri} permanent
}
EOF
fi

sed "s/{{VAPOR_DOMAIN}}/${VAPOR_DOMAIN}/g" \
  "${DEPLOY_DIR}/caddy/Caddyfile.template" \
  | sed \
    -e "/{{VAPOR_HTTP_FALLBACK_BLOCK}}/r ${fallback_block}" \
    -e "/{{VAPOR_HTTP_FALLBACK_BLOCK}}/d" \
  > /etc/caddy/Caddyfile

caddy fmt --overwrite /etc/caddy/Caddyfile
caddy validate --config /etc/caddy/Caddyfile
systemctl enable caddy.service

echo "caddy: installed config for ${VAPOR_DOMAIN}"
if [ -n "${VAPOR_HTTP_FALLBACK_HOST}" ]; then
  echo "caddy: installed canonical HTTP redirect for ${VAPOR_HTTP_FALLBACK_HOST}"
fi
