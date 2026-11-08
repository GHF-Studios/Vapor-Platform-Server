#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "${SCRIPT_DIR}/lib.sh"
require_root

if ! command -v apt-get >/dev/null 2>&1; then
  echo "error: emergency console setup currently requires Ubuntu/Debian apt" >&2
  exit 1
fi
if ! getent group caddy >/dev/null; then
  echo "error: expected the Caddy service group; install Caddy first" >&2
  exit 1
fi

if ! command -v ttyd >/dev/null 2>&1; then
  DEBIAN_FRONTEND=noninteractive apt-get update
  DEBIAN_FRONTEND=noninteractive apt-get install -y ttyd
fi

install -d -o root -g root -m 0755 /usr/local/libexec
install -o root -g root -m 0750 \
  "${SCRIPT_DIR}/vapor-emergency-console-shell" \
  /usr/local/libexec/vapor-emergency-console-shell
install -o root -g root -m 0644 \
  "${SCRIPT_DIR}/vapor-emergency-console.bashrc" \
  /usr/local/libexec/vapor-emergency-console.bashrc
install -o root -g root -m 0644 \
  "${DEPLOY_DIR}/systemd/vapor-emergency-console.service" \
  /etc/systemd/system/vapor-emergency-console.service
systemctl daemon-reload

echo "emergency-console: prepared; not enabled at boot or started"
echo "emergency-console: root-owned platform.env must set VAPOR_EMERGENCY_CONSOLE_ENABLED=1 before the admin page can enable a session"
echo "emergency-console: update the Caddy config from the matching Platform release before use"
