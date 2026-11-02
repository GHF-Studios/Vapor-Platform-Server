#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
DEPLOY_DIR="$(cd -- "${SCRIPT_DIR}/.." && pwd)"

export VAPOR_CADDY_TEMPLATE="${DEPLOY_DIR}/caddy/Caddyfile.services.template"
exec "${SCRIPT_DIR}/install-caddy.sh" "$@"
