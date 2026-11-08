#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "${SCRIPT_DIR}/lib.sh"
source "${SCRIPT_DIR}/ssh-expiry-quiescence.sh"

require_root
BUNDLE=""
CONFIRMED=false
while [ "$#" -gt 0 ]; do
  case "$1" in
    --bundle) [ "$#" -ge 2 ] || { echo "usage: restore-state-only.sh --bundle PATH --yes" >&2; exit 2; }; BUNDLE="$2"; shift 2 ;;
    --yes) CONFIRMED=true; shift ;;
    -h|--help) echo "usage: restore-state-only.sh --bundle PATH --yes"; exit 0 ;;
    *) echo "usage: restore-state-only.sh --bundle PATH --yes" >&2; exit 2 ;;
  esac
done
[ -n "${BUNDLE}" ] && [ "${CONFIRMED}" = true ] || { echo "usage: restore-state-only.sh --bundle PATH --yes" >&2; exit 2; }
[ -f "${BUNDLE}" ] && [ ! -L "${BUNDLE}" ] || { echo "error: archive is missing or unsafe" >&2; exit 1; }
python3 "${SCRIPT_DIR}/recovery_bundle_metadata.py" --check-archive-checksum "${BUNDLE}"
python3 "${SCRIPT_DIR}/state-bundle-validate.py" "${BUNDLE}"

STATE_ROOT="${VAPOR_STATE_ROOT:-/var/lib/vapor-server}"
BACKUP_ROOT="${VAPOR_BACKUP_ROOT:-/var/backups/vapor-server}"
[[ "${STATE_ROOT}" = /* && "${BACKUP_ROOT}" = /* ]] || { echo "error: state and backup roots must be absolute" >&2; exit 1; }
[ -d "${STATE_ROOT}" ] && [ ! -L "${STATE_ROOT}" ] || { echo "error: configured Vapor state root is not a real directory" >&2; exit 1; }
[ -f "${VAPOR_CONFIG_DIR}/platform.env" ] && [ ! -L "${VAPOR_CONFIG_DIR}/platform.env" ] || {
  echo "error: host setup is incomplete; the root-owned Platform configuration is missing" >&2
  exit 1
}
for required_unit in vapor-platform-server.service caddy.service; do
  systemctl cat "${required_unit}" >/dev/null 2>&1 || {
    echo "error: host setup is incomplete; required service unit is missing: ${required_unit}" >&2
    exit 1
  }
done
systemctl is-active --quiet vapor-platform-server.service || {
  echo "error: host setup is incomplete; Platform service is not active" >&2
  exit 1
}
systemctl is-active --quiet caddy.service || {
  echo "error: host setup is incomplete; HTTPS gateway is not active" >&2
  exit 1
}
if ! command -v flock >/dev/null 2>&1; then echo "error: flock is required for state restore locking" >&2; exit 1; fi
exec 9>/run/vapor-server-deploy.lock
if ! flock -n 9; then echo "error: Vapor deploy/state lock is already held; retry restore later" >&2; exit 1; fi

install_dir root root 0700 "${BACKUP_ROOT}"
TMP_DIR="$(mktemp -d "${BACKUP_ROOT}/.state-only-restore.XXXXXX")"
PRE_RESTORE="${BACKUP_ROOT}/pre-state-only-restore-$(date -u +%Y%m%dT%H%M%SZ)-$$"
install_dir root root 0700 "${PRE_RESTORE}"
tar -xzf "${BUNDLE}" -C "${TMP_DIR}"
BUNDLE_STATE="${TMP_DIR}/vapor-platform-recovery/state"
python3 "${SCRIPT_DIR}/validate-state-databases.py" "${BUNDLE_STATE}"
SERVICES=(
  vapor-homepage.service vapor-docs.service vapor-identity.service
  vapor-diagnostics.service vapor-registry.service vapor-artifact.service
  vapor-build-cache.service vapor-platform-server.service
)
TIMERS=(vapor-state-export.timer vapor-state-only-export.timer)
ACTIVE_SERVICES=()
ACTIVE_TIMERS=()
ACTIVE_SSH_EXPIRY_TIMERS=()
REPLACED_PATHS=()
MUTATION_STARTED=false
ROLLBACK_FAILED=false

restore_previous() {
  local relative
  local source target
  set +e
  for ((index=${#REPLACED_PATHS[@]}-1; index>=0; index--)); do
    relative="${REPLACED_PATHS[index]}"
    target="${STATE_ROOT}/${relative}"
    source="${PRE_RESTORE}/state/${relative}"
    rm -rf -- "${target}" || ROLLBACK_FAILED=true
    if [ -e "${source}" ] || [ -L "${source}" ]; then
      mkdir -p -- "$(dirname -- "${target}")" || ROLLBACK_FAILED=true
      mv -- "${source}" "${target}" || ROLLBACK_FAILED=true
    fi
  done
  set -e
  if [ "${ROLLBACK_FAILED}" = true ]; then
    echo "state-only-restore: rollback incomplete; preserved pre-restore state at ${PRE_RESTORE}" >&2
    return 1
  fi
}

cleanup() {
  local status=$?
  trap - EXIT
  set +e
  if [ "${status}" -ne 0 ] && [ "${MUTATION_STARTED}" = true ]; then
    for service in "${SERVICES[@]}"; do
      if systemctl is-active --quiet "${service}"; then systemctl stop "${service}" || ROLLBACK_FAILED=true; fi
    done
    for timer in "${TIMERS[@]}"; do
      if systemctl is-active --quiet "${timer}"; then systemctl stop "${timer}" || ROLLBACK_FAILED=true; fi
    done
    if pause_temporary_ssh_expiry; then
      restore_previous || ROLLBACK_FAILED=true
    else
      ROLLBACK_FAILED=true
    fi
    if [ "${ROLLBACK_FAILED}" = true ]; then
      ACTIVE_SERVICES=()
      ACTIVE_TIMERS=()
      status=1
    fi
  fi
  for service in "${ACTIVE_SERVICES[@]}"; do systemctl start "${service}" || status=1; done
  for timer in "${ACTIVE_TIMERS[@]}"; do systemctl start "${timer}" || status=1; done
  restart_temporary_ssh_expiry || status=1
  rm -rf -- "${TMP_DIR}"
  if [ "${status}" -eq 0 ]; then
    echo "state-only-restore: completed; pre-restore state retained at ${PRE_RESTORE}"
  elif [ "${ROLLBACK_FAILED}" = true ]; then
    echo "state-only-restore: failed and rollback incomplete; inspect ${PRE_RESTORE}" >&2
  else
    echo "state-only-restore: failed; previous state restored where available at ${PRE_RESTORE}" >&2
  fi
  exit "${status}"
}
trap cleanup EXIT

# Preserve only paths this restore may replace. Database WAL/SHM companions are
# included in the transaction so stale live journals cannot be replayed later.
REPLACE_PATHS=(
  identity/identity.sqlite3 identity/identity.sqlite3-wal identity/identity.sqlite3-shm
  platform/platform.sqlite3 platform/platform.sqlite3-wal platform/platform.sqlite3-shm
  registry/registry.sqlite3 registry/registry.sqlite3-wal registry/registry.sqlite3-shm
  docs/current docs/releases updates/releases operations/deployed-sources.json operations/latest.json
)
for relative in "${REPLACE_PATHS[@]}"; do
  source="${BUNDLE_STATE}/${relative}"
  case "${relative}" in
    identity/identity.sqlite3-wal|identity/identity.sqlite3-shm|platform/platform.sqlite3-wal|platform/platform.sqlite3-shm|registry/registry.sqlite3-wal|registry/registry.sqlite3-shm|operations/latest.json)
      ;;
    *) [ -e "${source}" ] || continue ;;
  esac
  path="${STATE_ROOT}/${relative}"
  parent="${STATE_ROOT}"
  IFS='/' read -r -a components <<< "${relative}"
  for ((i=0; i<${#components[@]}-1; i++)); do
    parent="${parent}/${components[i]}"
    [ ! -L "${parent}" ] || { echo "error: state path contains a symlink: ${relative}" >&2; exit 1; }
  done
done

for service in "${SERVICES[@]}"; do
  if systemctl is-active --quiet "${service}"; then ACTIVE_SERVICES+=("${service}"); systemctl stop "${service}"; fi
done
for timer in "${TIMERS[@]}"; do
  if systemctl is-active --quiet "${timer}"; then ACTIVE_TIMERS+=("${timer}"); systemctl stop "${timer}"; fi
done
pause_temporary_ssh_expiry

MUTATION_STARTED=true
# Mark every target before moving its old copy so a mid-transaction failure is
# able to restore every path already touched.
for relative in "${REPLACE_PATHS[@]}"; do
  source="${BUNDLE_STATE}/${relative}"
  target="${STATE_ROOT}/${relative}"
  case "${relative}" in
    identity/identity.sqlite3-wal|identity/identity.sqlite3-shm|platform/platform.sqlite3-wal|platform/platform.sqlite3-shm|registry/registry.sqlite3-wal|registry/registry.sqlite3-shm|operations/latest.json)
      ;;
    *) [ -e "${source}" ] || continue ;;
  esac
  REPLACED_PATHS+=("${relative}")
  if [ -e "${target}" ] || [ -L "${target}" ]; then
    mkdir -p -- "${PRE_RESTORE}/state/$(dirname -- "${relative}")"
    mv -- "${target}" "${PRE_RESTORE}/state/${relative}"
  fi
  rm -rf -- "${target}"
  if [ -e "${source}" ] || [ -L "${source}" ]; then
    mkdir -p -- "$(dirname -- "${target}")"
    cp -a -- "${source}" "${target}"
  fi
done

for path in identity platform registry docs updates operations; do
  if [ -e "${STATE_ROOT}/${path}" ]; then
    chown -R "${VAPOR_USER}:${VAPOR_GROUP}" "${STATE_ROOT}/${path}"
  fi
done
for service in "${ACTIVE_SERVICES[@]}"; do systemctl start "${service}"; done
for timer in "${ACTIVE_TIMERS[@]}"; do systemctl start "${timer}"; done
restart_temporary_ssh_expiry
ACTIVE_SSH_EXPIRY_TIMERS=()
"${SCRIPT_DIR}/health-check.sh"
echo "state-only-restore: services healthy after state import"
