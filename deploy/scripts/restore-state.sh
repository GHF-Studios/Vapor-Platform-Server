#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "${SCRIPT_DIR}/lib.sh"
source "${SCRIPT_DIR}/ssh-expiry-quiescence.sh"

require_root

BACKUP_ROOT="${VAPOR_BACKUP_ROOT:-/var/backups/vapor-server}"
BUNDLE_PATH=""
CONFIRMED="false"
FRESH_HOST_RESTORE="false"
ACTIVE_SERVICES=()
ENABLED_SERVICES=()
ACTIVE_SSH_EXPIRY_TIMERS=()
PRE_RESTORE_ACTIVE_SSH_EXPIRY_TIMERS=()
SSH_EXPIRY_QUIESCED="false"
STATEFUL_SERVICES=(
  vapor-homepage.service vapor-docs.service vapor-identity.service
  vapor-diagnostics.service vapor-registry.service vapor-artifact.service
  vapor-build-cache.service vapor-platform-server.service
)
TARGET_SERVICES=()
MODE=""
TMP_DIR=""
PRE_RESTORE_PATH=""
STATE_MOVED="false"
CONFIG_MOVED="false"
STATE_INSTALLED="false"
CONFIG_INSTALLED="false"
CADDY_INSTALLED="false"
UNITS_INSTALLED="false"
MUTATION_STARTED="false"
CADDY_WAS_ACTIVE="false"
CADDY_WAS_ENABLED="false"
TIMER_WAS_ENABLED="false"
TIMER_WAS_ACTIVE="false"
STATE_ONLY_TIMER_WAS_ENABLED="false"
STATE_ONLY_TIMER_WAS_ACTIVE="false"
DEPLOY_TIMER_WAS_ACTIVE="false"
DEPLOY_TIMER_WAS_ENABLED="false"
DEPLOY_SERVICE_WAS_ACTIVE="false"
DEPLOY_SERVICE_WAS_ENABLED="false"
ROLLBACK_RESULT="not_attempted"
RESTORE_STARTED_AT_UTC="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
RESTORE_REVISION="$(git -C "${VAPOR_DEPLOY_ROOT}" rev-parse HEAD 2>/dev/null || printf 'unknown')"

usage() {
  cat >&2 <<'USAGE'
usage: restore-state.sh --bundle PATH --yes [--fresh-host]

Restores a schema-v2 Vapor full-platform recovery bundle. It replaces service
state, server configuration, the Caddyfile, and Vapor systemd units. It keeps
the previous state/configuration under /var/backups/vapor-server/pre-restore-*. Use
--fresh-host when the old host's temporary SSH accounts and keys are not present.
USAGE
}

unit_load_state() {
  local unit="$1"
  local state
  if ! state="$(systemctl show --property=LoadState --value "${unit}" 2>/dev/null)"; then
    printf 'unavailable'
    return
  fi
  printf '%s' "${state:-unavailable}"
}

stop_if_loaded() {
  local unit="$1"
  local state
  state="$(unit_load_state "${unit}")"
  case "${state}" in
    loaded) systemctl stop "${unit}" ;;
    not-found) return 0 ;;
    *)
      echo "error: cannot safely stop ${unit}; systemd reports LoadState=${state}" >&2
      return 1
      ;;
  esac
}

restore_enabled_state_if_loaded() {
  local unit="$1"
  local enabled="$2"
  local state
  state="$(unit_load_state "${unit}")"
  case "${state}" in
    not-found) return 0 ;;
    loaded)
      if [ "${enabled}" = "true" ]; then
        systemctl enable "${unit}"
      else
        systemctl disable "${unit}" >/dev/null
      fi
      ;;
    *)
      echo "recovery-restore: cannot restore enabled state for ${unit}; systemd reports LoadState=${state}" >&2
      return 1
      ;;
  esac
}

activate_restored_ssh_expiry() {
  local timer
  local units
  local database="${VAPOR_PLATFORM_DB:-${VAPOR_PLATFORM_STATE:-${VAPOR_STATE_ROOT}/platform}/platform.sqlite3}"
  units="$(python3 "${SCRIPT_DIR}/restored_ssh_expiry_units.py" \
    "${database}" /etc/systemd/system)" || return 1
  while IFS= read -r timer; do
    [ -n "${timer}" ] || continue
    systemctl enable --now "${timer}" || return 1
  done <<< "${units}"
}

restart_previous_ssh_expiry() {
  local timer
  local failed=0
  for timer in "${ACTIVE_SSH_EXPIRY_TIMERS[@]}"; do
    if [ -f "/etc/systemd/system/${timer}" ] && \
      [ ! -L "/etc/systemd/system/${timer}" ] && \
      ! systemctl enable --now "${timer}"; then
      echo "recovery-restore: could not restart a pre-restore SSH expiry timer" >&2
      failed=1
    fi
  done
  return "${failed}"
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --bundle)
      if [ "$#" -lt 2 ]; then usage; exit 2; fi
      BUNDLE_PATH="$2"
      shift 2
      ;;
    --yes)
      CONFIRMED="true"
      shift
      ;;
    --fresh-host)
      FRESH_HOST_RESTORE="true"
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *) usage; exit 2 ;;
  esac
done

if [ -z "${BUNDLE_PATH}" ] || [ "${CONFIRMED}" != "true" ]; then
  usage
  exit 2
fi
if [ ! -f "${BUNDLE_PATH}" ]; then
  echo "error: recovery bundle does not exist" >&2
  exit 1
fi
if ! command -v flock >/dev/null 2>&1; then
  echo "error: flock is required for state restore locking" >&2
  exit 1
fi

python3 "${SCRIPT_DIR}/recovery_bundle_metadata.py" --check-archive-checksum "${BUNDLE_PATH}"
python3 "${SCRIPT_DIR}/state-bundle-validate.py" "${BUNDLE_PATH}"
MODE="$(python3 - "${BUNDLE_PATH}" <<'PY'
import sys
import tarfile
import tomllib

with tarfile.open(sys.argv[1], "r:gz") as archive:
    member = archive.getmember("vapor-platform-recovery/manifest.toml")
    stream = archive.extractfile(member)
    if stream is None:
        raise SystemExit("recovery manifest unavailable")
    manifest = tomllib.loads(stream.read().decode("utf-8"))
mode = manifest.get("runtime_mode")
if mode not in {"monolith", "services"}:
    raise SystemExit("unsupported runtime mode")
print(mode)
PY
)"
VAPOR_SERVER_MODE="${MODE}"
export VAPOR_SERVER_MODE

case "${MODE}" in
  monolith)
    TARGET_SERVICES=(vapor-platform-server.service)
    REQUIRED_BINARIES=(vapor_platform_server)
    ;;
  services)
    TARGET_SERVICES=(
      vapor-homepage.service vapor-docs.service vapor-identity.service
      vapor-diagnostics.service vapor-registry.service vapor-artifact.service
      vapor-build-cache.service vapor-platform-server.service
    )
    REQUIRED_BINARIES=(
      vapor_homepage_server vapor_docs_server vapor_identity_server
      vapor_diagnostics_server vapor_registry_server vapor_artifact_server
      vapor_build_cache_server vapor_platform_server
    )
    ;;
  *) echo "error: unsupported recovery runtime mode" >&2; exit 1 ;;
esac

LOCK_FILE=/run/vapor-server-deploy.lock
exec 9>"${LOCK_FILE}"
if ! flock -n 9; then
  echo "error: Vapor deploy/state lock is already held; retry after the active operation finishes" >&2
  exit 1
fi
if ! python3 - "${BUNDLE_PATH}" "${VAPOR_DEPLOY_ROOT}" <<'PY'
import subprocess
import sys
import tarfile

bundle, source_root = sys.argv[1:]
try:
    with tarfile.open(bundle, "r:gz") as archive:
        expected_commit = archive.extractfile(
            "vapor-platform-recovery/source/root-commit.txt"
        ).read(256).decode("ascii").strip()
        expected_submodules = archive.extractfile(
            "vapor-platform-recovery/source/submodules.txt"
        ).read(4 * 1024 * 1024).decode("utf-8")
    actual_commit = subprocess.check_output(
        ["git", "-C", source_root, "rev-parse", "HEAD"], text=True
    ).strip()
    actual_submodules = subprocess.check_output(
        ["git", "-C", source_root, "submodule", "status", "--recursive"],
        text=True,
    )
    source_status = subprocess.check_output(
        ["git", "-C", source_root, "status", "--porcelain", "--untracked-files=all"],
        text=True,
    )
    submodule_status = subprocess.run(
        [
            "git", "-C", source_root, "submodule", "foreach", "--quiet", "--recursive",
            'test -z "$(git status --porcelain --untracked-files=all)"',
        ],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=False,
    )
except (OSError, subprocess.CalledProcessError, tarfile.TarError, AttributeError):
    raise SystemExit("recovery-restore: source pin recheck failed")
if actual_commit != expected_commit or actual_submodules != expected_submodules:
    raise SystemExit("recovery-restore: source checkout changed before restore acquired the lock")
if source_status or submodule_status.returncode != 0:
    raise SystemExit("recovery-restore: source checkout is dirty")
PY
then
  exit 1
fi
for binary in "${REQUIRED_BINARIES[@]}"; do
  if [ ! -x "${VAPOR_DEPLOY_ROOT}/target/release/${binary}" ]; then
    echo "error: a required service binary is missing; build the recorded source before restore" >&2
    exit 1
  fi
done
python3 "${SCRIPT_DIR}/restore_host_preflight.py" || exit 1

install_dir root root 0700 "${BACKUP_ROOT}"
TMP_DIR="$(mktemp -d "${BACKUP_ROOT}/.restore.XXXXXX")"
PRE_RESTORE_PATH="$(mktemp -d "${BACKUP_ROOT}/pre-restore-$(date -u +%Y%m%dT%H%M%SZ).XXXXXX")"
chown root:root "${PRE_RESTORE_PATH}"
chmod 0700 "${PRE_RESTORE_PATH}"
tar -xzf "${BUNDLE_PATH}" -C "${TMP_DIR}"
BUNDLE_ROOT="${TMP_DIR}/vapor-platform-recovery"

if [ "${FRESH_HOST_RESTORE}" = "true" ]; then
  python3 "${SCRIPT_DIR}/reconcile-fresh-host-ssh.py" \
    "${BUNDLE_ROOT}/state/platform/platform.sqlite3"
fi

for service in "${STATEFUL_SERVICES[@]}"; do
  if systemctl is-active --quiet "${service}"; then
    ACTIVE_SERVICES+=("${service}")
  fi
  if systemctl is-enabled --quiet "${service}"; then
    ENABLED_SERVICES+=("${service}")
  fi
done
if systemctl is-active --quiet caddy.service; then
  CADDY_WAS_ACTIVE="true"
fi
if systemctl is-enabled --quiet caddy.service; then CADDY_WAS_ENABLED="true"; fi
if systemctl is-enabled --quiet vapor-state-export.timer; then TIMER_WAS_ENABLED="true"; fi
if systemctl is-active --quiet vapor-state-export.timer; then
  TIMER_WAS_ACTIVE="true"
fi
if systemctl is-enabled --quiet vapor-state-only-export.timer; then STATE_ONLY_TIMER_WAS_ENABLED="true"; fi
if systemctl is-active --quiet vapor-state-only-export.timer; then STATE_ONLY_TIMER_WAS_ACTIVE="true"; fi
if systemctl is-enabled --quiet vapor-deploy.timer; then DEPLOY_TIMER_WAS_ENABLED="true"; fi
if systemctl is-active --quiet vapor-deploy.timer; then DEPLOY_TIMER_WAS_ACTIVE="true"; fi
if systemctl is-enabled --quiet vapor-deploy.service; then DEPLOY_SERVICE_WAS_ENABLED="true"; fi
if systemctl is-active --quiet vapor-deploy.service; then DEPLOY_SERVICE_WAS_ACTIVE="true"; fi

restore_previous_host_files() {
  mkdir -p /etc/systemd/system || return 1
  find /etc/systemd/system -maxdepth 1 \( -type f -o -type l \) \
    \( -name 'vapor-*.service' -o -name 'vapor-*.timer' -o -name 'vapor-*.socket' \) -delete || return 1
  if [ -d "${PRE_RESTORE_PATH}/host/systemd" ]; then
    cp -a "${PRE_RESTORE_PATH}/host/systemd/." /etc/systemd/system/ || return 1
  fi
  if [ -f "${PRE_RESTORE_PATH}/host/caddy/Caddyfile" ]; then
    install -D -o root -g root -m 0644 \
      "${PRE_RESTORE_PATH}/host/caddy/Caddyfile" /etc/caddy/Caddyfile || return 1
  elif [ -e /etc/caddy/Caddyfile ]; then
    rm -f /etc/caddy/Caddyfile || return 1
  fi
  systemctl daemon-reload || return 1
  if [ "${CADDY_WAS_ACTIVE}" = "true" ]; then
    systemctl reload caddy.service || systemctl restart caddy.service || return 1
  fi
}

rollback() {
  local service
  local enabled
  local was_enabled
  local rollback_failed=0
  local -a previous_timers=()
  set +e
  if [ "${MUTATION_STARTED}" != "true" ]; then return; fi
  ROLLBACK_RESULT="incomplete"
  for service in "${STATEFUL_SERVICES[@]}"; do
    stop_if_loaded "${service}" || rollback_failed=1
  done
  if [ "${rollback_failed}" -ne 0 ]; then
    echo "recovery-restore: rollback incomplete; could not stop every service" >&2
    return 1
  fi
  previous_timers=("${PRE_RESTORE_ACTIVE_SSH_EXPIRY_TIMERS[@]}")
  ACTIVE_SSH_EXPIRY_TIMERS=()
  if ! pause_temporary_ssh_expiry; then
    echo "recovery-restore: rollback stopped before state replacement because SSH expiry work could not be quiesced" >&2
    return 1
  fi
  if [ "${SSH_EXPIRY_QUIESCED}" != "true" ]; then
    previous_timers+=("${ACTIVE_SSH_EXPIRY_TIMERS[@]}")
  fi
  ACTIVE_SSH_EXPIRY_TIMERS=("${previous_timers[@]}")
  if [ "${STATE_INSTALLED}" = "true" ] && [ -e "${VAPOR_STATE_ROOT}" ]; then
    rm -rf -- "${VAPOR_STATE_ROOT}" || rollback_failed=1
  fi
  if [ "${STATE_MOVED}" = "true" ] && [ -e "${PRE_RESTORE_PATH}/state" ]; then
    mv -- "${PRE_RESTORE_PATH}/state" "${VAPOR_STATE_ROOT}" || rollback_failed=1
  fi
  if [ "${CONFIG_INSTALLED}" = "true" ] && [ -e "${VAPOR_CONFIG_DIR}" ]; then
    rm -rf -- "${VAPOR_CONFIG_DIR}" || rollback_failed=1
  fi
  if [ "${CONFIG_MOVED}" = "true" ] && [ -e "${PRE_RESTORE_PATH}/config/vapor-server" ]; then
    mv -- "${PRE_RESTORE_PATH}/config/vapor-server" "${VAPOR_CONFIG_DIR}" || rollback_failed=1
  fi
  if [ "${CADDY_INSTALLED}" = "true" ] || [ "${UNITS_INSTALLED}" = "true" ]; then
    restore_previous_host_files || rollback_failed=1
  fi
  if [ "${rollback_failed}" -ne 0 ]; then
    echo "recovery-restore: rollback incomplete; restored services will remain stopped" >&2
    return 1
  fi
  for service in "${STATEFUL_SERVICES[@]}"; do
    was_enabled="false"
    for enabled in "${ENABLED_SERVICES[@]}"; do
      if [ "${enabled}" = "${service}" ]; then was_enabled="true"; fi
    done
    restore_enabled_state_if_loaded "${service}" "${was_enabled}" || rollback_failed=1
  done
  restore_enabled_state_if_loaded caddy.service "${CADDY_WAS_ENABLED}" || rollback_failed=1
  restore_enabled_state_if_loaded vapor-state-export.timer "${TIMER_WAS_ENABLED}" || rollback_failed=1
  if [ "${TIMER_WAS_ACTIVE}" = "true" ]; then
    systemctl start vapor-state-export.timer || rollback_failed=1
  fi
  restore_enabled_state_if_loaded vapor-state-only-export.timer "${STATE_ONLY_TIMER_WAS_ENABLED}" || rollback_failed=1
  if [ "${STATE_ONLY_TIMER_WAS_ACTIVE}" = "true" ]; then
    systemctl start vapor-state-only-export.timer || rollback_failed=1
  fi
  restore_enabled_state_if_loaded vapor-deploy.timer "${DEPLOY_TIMER_WAS_ENABLED}" || rollback_failed=1
  if [ "${DEPLOY_TIMER_WAS_ACTIVE}" = "true" ]; then
    systemctl start vapor-deploy.timer || rollback_failed=1
  fi
  restore_enabled_state_if_loaded vapor-deploy.service "${DEPLOY_SERVICE_WAS_ENABLED}" || rollback_failed=1
  if [ "${DEPLOY_SERVICE_WAS_ACTIVE}" = "true" ]; then
    systemctl start vapor-deploy.service || rollback_failed=1
  fi
  if [ "${rollback_failed}" -ne 0 ]; then
    echo "recovery-restore: rollback incomplete; service restart was skipped" >&2
    return 1
  fi
  for service in "${ACTIVE_SERVICES[@]}"; do systemctl start "${service}" || rollback_failed=1; done
  if [ "${CADDY_WAS_ACTIVE}" = "true" ]; then systemctl start caddy.service || rollback_failed=1; fi
  if [ "${rollback_failed}" -eq 0 ]; then
    restart_previous_ssh_expiry || rollback_failed=1
  fi
  if [ "${rollback_failed}" -eq 0 ]; then
    ROLLBACK_RESULT="complete"
    echo "recovery-restore: rollback completed" >&2
  else
    echo "recovery-restore: rollback incomplete; inspect service logs and preserved pre-restore files" >&2
  fi
  return "${rollback_failed}"
}

cleanup_and_record() {
  local status=$?
  trap - EXIT
  set +e
  if [ "${status}" -ne 0 ]; then
    if ! rollback; then
      ROLLBACK_RESULT="incomplete"
    fi
    echo "recovery-restore: operation failed (rollback ${ROLLBACK_RESULT})" >&2
  fi
  local result=success
  [ "${status}" -eq 0 ] || result=failed
  local finished_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  write_operation_record restore "${result}" "${RESTORE_STARTED_AT_UTC}" \
    "${finished_at}" "${RESTORE_REVISION}" "${MODE}" || true
  rm -rf -- "${TMP_DIR}"
  exit "${status}"
}
trap cleanup_and_record EXIT

MUTATION_STARTED="true"
if [ "${DEPLOY_TIMER_WAS_ACTIVE}" = "true" ]; then
  systemctl stop vapor-deploy.timer
fi
systemctl disable vapor-deploy.timer >/dev/null 2>&1 || true
stop_if_loaded vapor-deploy.service
stop_if_loaded vapor-state-export.timer
stop_if_loaded vapor-state-only-export.timer
systemctl disable vapor-state-only-export.timer >/dev/null 2>&1 || true
for service in "${STATEFUL_SERVICES[@]}"; do stop_if_loaded "${service}"; done
pause_temporary_ssh_expiry
PRE_RESTORE_ACTIVE_SSH_EXPIRY_TIMERS=("${ACTIVE_SSH_EXPIRY_TIMERS[@]}")
SSH_EXPIRY_QUIESCED="true"

mkdir -p "${PRE_RESTORE_PATH}/host/caddy" "${PRE_RESTORE_PATH}/host/systemd" \
  "${PRE_RESTORE_PATH}/config"
if [ -e "${VAPOR_STATE_ROOT}" ]; then
  mv -- "${VAPOR_STATE_ROOT}" "${PRE_RESTORE_PATH}/state"
  STATE_MOVED="true"
fi
if [ -e "${VAPOR_CONFIG_DIR}" ]; then
  mv -- "${VAPOR_CONFIG_DIR}" "${PRE_RESTORE_PATH}/config/vapor-server"
  CONFIG_MOVED="true"
fi
if [ -f /etc/caddy/Caddyfile ]; then
  cp -a /etc/caddy/Caddyfile "${PRE_RESTORE_PATH}/host/caddy/Caddyfile"
fi
for unit in /etc/systemd/system/vapor-*.service /etc/systemd/system/vapor-*.timer /etc/systemd/system/vapor-*.socket; do
  [ -f "${unit}" ] && [ ! -L "${unit}" ] || continue
  cp -a "${unit}" "${PRE_RESTORE_PATH}/host/systemd/"
done

STATE_INSTALLED="true"
install_dir "${VAPOR_USER}" "${VAPOR_GROUP}" 0750 "${VAPOR_STATE_ROOT}"
cp -a "${BUNDLE_ROOT}/state/." "${VAPOR_STATE_ROOT}/"
chown -R "${VAPOR_USER}:${VAPOR_GROUP}" "${VAPOR_STATE_ROOT}"
find "${VAPOR_STATE_ROOT}" -type d -exec chmod 0750 {} +
find "${VAPOR_STATE_ROOT}" -type f -exec chmod 0640 {} +

CONFIG_INSTALLED="true"
install_dir root "${VAPOR_GROUP}" 0750 "${VAPOR_CONFIG_DIR}"
cp -a "${BUNDLE_ROOT}/config/vapor-server/." "${VAPOR_CONFIG_DIR}/"
chown -R "root:${VAPOR_GROUP}" "${VAPOR_CONFIG_DIR}"
find "${VAPOR_CONFIG_DIR}" -type d -exec chmod 0750 {} +
find "${VAPOR_CONFIG_DIR}" -type f -exec chmod 0640 {} +

CADDY_INSTALLED="true"
install -D -o root -g root -m 0644 \
  "${BUNDLE_ROOT}/host/caddy/Caddyfile" /etc/caddy/Caddyfile
UNITS_INSTALLED="true"
find /etc/systemd/system -maxdepth 1 \( -type f -o -type l \) \
  \( -name 'vapor-*.service' -o -name 'vapor-*.timer' -o -name 'vapor-*.socket' \) -delete
for unit in "${BUNDLE_ROOT}"/host/systemd/vapor-*; do
  [ -f "${unit}" ] || continue
  if [ "${FRESH_HOST_RESTORE}" = "true" ]; then
    case "$(basename -- "${unit}")" in
      vapor-ssh-expire-*.service|vapor-ssh-expire-*.timer) continue ;;
    esac
  fi
  install -D -o root -g root -m 0644 "${unit}" "/etc/systemd/system/$(basename -- "${unit}")"
done
# Legacy schema-v2 archives predate the separately indexed state-only timer.
# Reinstall the current setup-owned unit definitions instead of restoring
# obsolete host setup from the archive.
for unit in vapor-state-only-export.service vapor-state-only-export.timer; do
  if [ -f "${VAPOR_DEPLOY_ROOT}/deploy/systemd/${unit}" ]; then
    install -D -o root -g root -m 0644 \
      "${VAPOR_DEPLOY_ROOT}/deploy/systemd/${unit}" "/etc/systemd/system/${unit}"
  fi
done
for link in /etc/systemd/system/timers.target.wants/vapor-ssh-expire-*.timer; do
  [ -L "${link}" ] || continue
  rm -- "${link}"
done

systemctl daemon-reload
caddy validate --config /etc/caddy/Caddyfile
systemctl enable --now caddy.service
systemctl enable --now vapor-platform-ops.socket
for service in "${STATEFUL_SERVICES[@]}"; do
  if [[ ! " ${TARGET_SERVICES[*]} " =~ " ${service} " ]]; then
    systemctl disable --now "${service}" >/dev/null 2>&1 || true
  fi
done
for service in "${TARGET_SERVICES[@]}"; do
  systemctl enable --now "${service}"
done
if [ "${FRESH_HOST_RESTORE}" != "true" ]; then
  activate_restored_ssh_expiry
fi
systemctl disable --now vapor-state-export.timer >/dev/null 2>&1 || true
systemctl enable --now vapor-state-only-export.timer
systemctl reload caddy.service

"${SCRIPT_DIR}/health-check.sh"
"${SCRIPT_DIR}/retire-platform-ops-sudoers.sh"
echo "recovery-restore: restored validated full-platform bundle"
echo "recovery-restore: previous state/configuration retained at ${PRE_RESTORE_PATH}"
