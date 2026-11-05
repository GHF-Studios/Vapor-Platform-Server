#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/lib.sh"
require_root

if ! command -v flock >/dev/null 2>&1; then
  echo "error: flock is required for approved deploy serialization" >&2
  exit 1
fi

legacy_timer=/etc/systemd/system/vapor-deploy.timer
legacy_service=/etc/systemd/system/vapor-deploy.service
if [ -e "${legacy_timer}" ] && { [ ! -f "${legacy_timer}" ] || [ -L "${legacy_timer}" ]; }; then
  echo "error: unexpected legacy timer file; refusing to remove it" >&2
  exit 1
fi
if [ -e "${legacy_service}" ] && { [ ! -f "${legacy_service}" ] || [ -L "${legacy_service}" ]; }; then
  echo "error: unexpected legacy service file; refusing to remove it" >&2
  exit 1
fi
install -m 0644 "${DEPLOY_DIR}/systemd/vapor-deploy@.service" /etc/systemd/system/vapor-deploy@.service
if systemctl is-active --quiet vapor-deploy.timer; then
  systemctl stop vapor-deploy.timer
fi
if systemctl is-enabled --quiet vapor-deploy.timer; then
  systemctl disable vapor-deploy.timer
fi
if systemctl is-active --quiet vapor-deploy.service; then
  echo "error: the legacy deploy service is still running; let it finish, then rerun this installer" >&2
  exit 1
fi
rm -f -- "${legacy_timer}" "${legacy_service}"
systemctl daemon-reload
if systemctl is-active --quiet vapor-deploy.timer; then
  echo "error: the retired deploy timer is still active" >&2
  exit 1
fi

echo "approved-operations: request-scoped deploy units installed; periodic deploy timer removed"
