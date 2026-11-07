#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "${SCRIPT_DIR}/lib.sh"

require_root

for service in homepage docs identity diagnostics registry artifact build-cache; do
  install -m 0644 "${DEPLOY_DIR}/systemd/vapor-${service}.service" \
    "/etc/systemd/system/vapor-${service}.service"
done
install -m 0644 "${DEPLOY_DIR}/systemd/vapor-platform-gateway.service" \
  /etc/systemd/system/vapor-platform-server.service

systemctl daemon-reload
for service in homepage docs identity diagnostics registry artifact build-cache platform-server; do
  systemctl enable "vapor-${service}.service"
done

echo "systemd: installed and enabled independent Vapor service units"
