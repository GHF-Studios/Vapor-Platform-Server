#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "${SCRIPT_DIR}/lib.sh"

require_root

install -m 0644 "${DEPLOY_DIR}/systemd/vapor-platform-server.service" /etc/systemd/system/vapor-platform-server.service

systemctl daemon-reload

systemctl enable vapor-platform-server.service

echo "systemd: installed and enabled the composed Vapor Platform Server process"
