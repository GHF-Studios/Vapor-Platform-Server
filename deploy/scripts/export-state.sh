#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "${SCRIPT_DIR}/lib.sh"

require_root

BACKUP_ROOT="${VAPOR_BACKUP_ROOT:-/var/backups/vapor-server}"
BACKUP_RETENTION_COUNT="${VAPOR_BACKUP_RETENTION_COUNT:-48}"
TIMESTAMP="$(date -u +%Y%m%dT%H%M%SZ)"
DEPLOY_COMMIT="$(git -C "${VAPOR_DEPLOY_ROOT}" rev-parse --short HEAD 2>/dev/null || printf 'unknown')"
BUNDLE_PATH="${1:-${BACKUP_ROOT}/vapor-server-state-${TIMESTAMP}-${DEPLOY_COMMIT}.tar.gz}"
STATEFUL_SERVICES=(
  vapor-docs.service
  vapor-identity.service
  vapor-diagnostics.service
  vapor-platform-server.service
  vapor-build-cache.service
)
ACTIVE_SERVICES=()

if [ -e "${BUNDLE_PATH}" ]; then
  echo "error: export target already exists: ${BUNDLE_PATH}" >&2
  exit 1
fi

if ! command -v flock >/dev/null 2>&1; then
  echo "error: flock is required for state export locking" >&2
  exit 1
fi

case "${BACKUP_RETENTION_COUNT}" in
  ""|*[!0-9]*)
    echo "error: VAPOR_BACKUP_RETENTION_COUNT must be a positive integer" >&2
    exit 1
    ;;
esac
if [ "${BACKUP_RETENTION_COUNT}" -lt 1 ]; then
  echo "error: VAPOR_BACKUP_RETENTION_COUNT must be at least 1" >&2
  exit 1
fi

install_dir root root 0750 "${BACKUP_ROOT}"
BUNDLE_DIR="$(dirname -- "${BUNDLE_PATH}")"
if [ "${BUNDLE_DIR}" != "." ]; then
  install_dir root root 0750 "${BUNDLE_DIR}"
fi

LOCK_FILE=/run/vapor-server-deploy.lock
exec 9>"${LOCK_FILE}"
if ! flock -n 9; then
  echo "error: Vapor deploy/state lock is already held; retry after the active operation finishes" >&2
  exit 1
fi

TMP_DIR="$(mktemp -d)"
BUNDLE_ROOT="${TMP_DIR}/vapor-server-state"

cleanup() {
  rm -rf "${TMP_DIR}"
}

restart_services() {
  local service
  for service in "${ACTIVE_SERVICES[@]}"; do
    systemctl start "${service}"
  done
}

trap 'status=$?; set +e; restart_services; cleanup; exit ${status}' EXIT

for service in "${STATEFUL_SERVICES[@]}"; do
  if systemctl is-active --quiet "${service}"; then
    ACTIVE_SERVICES+=("${service}")
    systemctl stop "${service}"
  fi
done

install -d -o root -g root -m 0750 "${BUNDLE_ROOT}/state"
if [ -d "${VAPOR_STATE_ROOT}" ]; then
  cp -a "${VAPOR_STATE_ROOT}/." "${BUNDLE_ROOT}/state/"
fi

{
  printf 'schema_version = 1\n'
  printf 'created_at_utc = "%s"\n' "${TIMESTAMP}"
  printf 'secrets_included = false\n'
  printf 'deploy_root = "%s"\n' "${VAPOR_DEPLOY_ROOT}"
  printf 'state_root = "%s"\n' "${VAPOR_STATE_ROOT}"
  printf 'config_dir_excluded = "%s"\n' "${VAPOR_CONFIG_DIR}"
  printf 'vapor_server_root_commit = "%s"\n' "${DEPLOY_COMMIT}"
  printf 'vapor_branch = "%s"\n' "${VAPOR_BRANCH}"
} > "${BUNDLE_ROOT}/manifest.toml"

git -C "${VAPOR_DEPLOY_ROOT}" submodule status --recursive > "${BUNDLE_ROOT}/submodules.txt" 2>/dev/null || true

restart_services
ACTIVE_SERVICES=()

tar -C "${TMP_DIR}" -czf "${BUNDLE_PATH}" vapor-server-state
chmod 0600 "${BUNDLE_PATH}"

backup_count="$(find "${BACKUP_ROOT}" -maxdepth 1 -type f -name 'vapor-server-state-*.tar.gz' | wc -l)"
if [ "${backup_count}" -gt "${BACKUP_RETENTION_COUNT}" ]; then
  find "${BACKUP_ROOT}" -maxdepth 1 -type f -name 'vapor-server-state-*.tar.gz' -printf '%T@ %p\n' |
    sort -nr |
    sed -n "$((BACKUP_RETENTION_COUNT + 1)),\$p" |
    while IFS= read -r line; do
      old_backup="${line#* }"
      if [ -n "${old_backup}" ] && [ -f "${old_backup}" ]; then
        rm -f -- "${old_backup}"
        echo "state-export: pruned ${old_backup}"
      fi
    done
fi

echo "state-export: wrote ${BUNDLE_PATH}"
