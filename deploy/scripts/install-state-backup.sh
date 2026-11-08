#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "${SCRIPT_DIR}/lib.sh"

require_root

install -m 0644 "${DEPLOY_DIR}/systemd/vapor-state-export.service" /etc/systemd/system/vapor-state-export.service
install -m 0644 "${DEPLOY_DIR}/systemd/vapor-state-export.timer" /etc/systemd/system/vapor-state-export.timer
install -m 0644 "${DEPLOY_DIR}/systemd/vapor-state-only-export.service" /etc/systemd/system/vapor-state-only-export.service
install -m 0644 "${DEPLOY_DIR}/systemd/vapor-state-only-export.timer" /etc/systemd/system/vapor-state-only-export.timer

systemctl daemon-reload
if [ "${VAPOR_RECOVERY_STAGING:-0}" != "1" ]; then
  # New exports contain only allowlisted product state. Stop generating the
  # legacy full-host archives, which include server configuration and secrets.
  systemctl disable --now vapor-state-export.timer >/dev/null 2>&1 || true
  systemctl enable --now vapor-state-only-export.timer
fi

if [ "${VAPOR_RECOVERY_STAGING:-0}" = "1" ]; then
  echo "state-backup: installed state export timers without starting them for recovery staging"
else
  echo "state-backup: legacy full-host exports disabled; state-only export enabled"
fi
