#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "${SCRIPT_DIR}/lib.sh"

require_root

install -m 0644 "${DEPLOY_DIR}/systemd/vapor-state-export.service" /etc/systemd/system/vapor-state-export.service
install -m 0644 "${DEPLOY_DIR}/systemd/vapor-state-export.timer" /etc/systemd/system/vapor-state-export.timer

systemctl daemon-reload
if [ "${VAPOR_RECOVERY_STAGING:-0}" != "1" ]; then
  systemctl enable --now vapor-state-export.timer
fi

if [ "${VAPOR_RECOVERY_STAGING:-0}" = "1" ]; then
  echo "state-backup: installed vapor-state-export.timer without starting it for recovery staging"
else
  echo "state-backup: installed and enabled vapor-state-export.timer"
fi
