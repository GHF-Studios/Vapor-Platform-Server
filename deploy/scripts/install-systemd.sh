#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "${SCRIPT_DIR}/lib.sh"

require_root

install -m 0644 "${DEPLOY_DIR}/systemd/vapor-platform-server.service" /etc/systemd/system/vapor-platform-server.service

systemctl daemon-reload

if [ "${VAPOR_RECOVERY_STAGING:-0}" = "1" ]; then
  echo "systemd: installed the composed Platform unit without enabling it for recovery staging"
  exit 0
fi

systemctl enable vapor-platform-server.service

echo "systemd: installed and enabled the composed Vapor Platform Server process"
