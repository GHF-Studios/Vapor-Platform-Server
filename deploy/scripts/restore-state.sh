#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "${SCRIPT_DIR}/lib.sh"

require_root

BACKUP_ROOT="${VAPOR_BACKUP_ROOT:-/var/backups/vapor-server}"
BUNDLE_PATH=""
CONFIRMED="false"
STATEFUL_SERVICES=(
  vapor-docs.service
  vapor-identity.service
  vapor-diagnostics.service
  vapor-platform-server.service
  vapor-build-cache.service
)
ACTIVE_SERVICES=()
TIMER_WAS_ACTIVE="false"

usage() {
  cat >&2 <<'USAGE'
usage: restore-state.sh --bundle PATH --yes

Restores a Vapor server state bundle created by export-state.sh.
This replaces /var/lib/vapor-server state, moves the previous state aside under
/var/backups/vapor-server/pre-restore-*, and does not restore /etc/vapor-server
env/token files.
USAGE
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --bundle)
      if [ "$#" -lt 2 ]; then
        usage
        exit 2
      fi
      BUNDLE_PATH="$2"
      shift 2
      ;;
    --yes)
      CONFIRMED="true"
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      usage
      exit 2
      ;;
  esac
done

if [ -z "${BUNDLE_PATH}" ] || [ "${CONFIRMED}" != "true" ]; then
  usage
  exit 2
fi

if [ ! -f "${BUNDLE_PATH}" ]; then
  echo "error: bundle does not exist: ${BUNDLE_PATH}" >&2
  exit 1
fi

if ! command -v flock >/dev/null 2>&1; then
  echo "error: flock is required for state restore locking" >&2
  exit 1
fi

python3 "${SCRIPT_DIR}/state-bundle-validate.py" "${BUNDLE_PATH}"

LOCK_FILE=/run/vapor-server-deploy.lock
exec 9>"${LOCK_FILE}"
if ! flock -n 9; then
  echo "error: Vapor deploy/state lock is already held; retry after the active operation finishes" >&2
  exit 1
fi

TMP_DIR="$(mktemp -d)"
RESTORE_TIMESTAMP="$(date -u +%Y%m%dT%H%M%SZ)"
PRE_RESTORE_PATH="${BACKUP_ROOT}/pre-restore-${RESTORE_TIMESTAMP}"
RESTORE_OPERATION_STARTED_AT_UTC="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
RESTORE_OPERATION_REVISION="$(git -C "${VAPOR_DEPLOY_ROOT}" rev-parse HEAD 2>/dev/null || printf 'unknown')"

cleanup() {
  rm -rf "${TMP_DIR}"
}

restart_services() {
  local service
  for service in "${ACTIVE_SERVICES[@]}"; do
    systemctl start "${service}"
  done
  if [ "${TIMER_WAS_ACTIVE}" = "true" ]; then
    systemctl start vapor-deploy.timer
  fi
}

record_restore_operation() {
  local status="$?"
  local result="failed"
  local finished_at_utc
  trap - EXIT
  set +e
  if [ "$status" -eq 0 ]; then
    result="success"
  fi
  restart_services
  cleanup
  finished_at_utc="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  if ! write_operation_record \
    restore "$result" "$RESTORE_OPERATION_STARTED_AT_UTC" "$finished_at_utc" \
    "$RESTORE_OPERATION_REVISION" "$VAPOR_SERVER_MODE"; then
    echo "warning: restore result could not be recorded" >&2
  fi
  exit "$status"
}
trap record_restore_operation EXIT

if systemctl is-active --quiet vapor-deploy.timer; then
  TIMER_WAS_ACTIVE="true"
  systemctl stop vapor-deploy.timer
fi
systemctl stop vapor-deploy.service 2>/dev/null || true

for service in "${STATEFUL_SERVICES[@]}"; do
  if systemctl is-active --quiet "${service}"; then
    ACTIVE_SERVICES+=("${service}")
    systemctl stop "${service}"
  fi
done

tar -xzf "${BUNDLE_PATH}" -C "${TMP_DIR}"

install_dir root root 0750 "${BACKUP_ROOT}"
if [ -e "${VAPOR_STATE_ROOT}" ]; then
  mv "${VAPOR_STATE_ROOT}" "${PRE_RESTORE_PATH}"
fi

install_dir "${VAPOR_USER}" "${VAPOR_GROUP}" 0750 "${VAPOR_STATE_ROOT}"
cp -a "${TMP_DIR}/vapor-server-state/state/." "${VAPOR_STATE_ROOT}/"
chown -R "${VAPOR_USER}:${VAPOR_GROUP}" "${VAPOR_STATE_ROOT}"
find "${VAPOR_STATE_ROOT}" -type d -exec chmod 0750 {} +
find "${VAPOR_STATE_ROOT}" -type f -exec chmod 0640 {} +

restart_services
ACTIVE_SERVICES=()
TIMER_WAS_ACTIVE="false"

"${SCRIPT_DIR}/health-check.sh"

echo "state-restore: restored ${BUNDLE_PATH}"
if [ -e "${PRE_RESTORE_PATH}" ]; then
  echo "state-restore: previous state moved to ${PRE_RESTORE_PATH}"
fi
