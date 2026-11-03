#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/lib.sh"
require_root

DATABASE="${VAPOR_PLATFORM_DB:-${VAPOR_PLATFORM_STATE:-${VAPOR_STATE_ROOT}/platform}/platform.sqlite3}"
if [ -f "${DATABASE}" ]; then
  python3 - "${DATABASE}" <<'PY'
import sqlite3, sys
with sqlite3.connect(sys.argv[1]) as db:
    row = db.execute(
        "SELECT COUNT(*) FROM platform_ssh_access_requests "
        "WHERE status IN ('active','provisioning','failed')"
    ).fetchone()
if row[0]:
    raise SystemExit("active or incomplete SSH grants exist; revoke/clean them before uninstall")
PY
fi

MANAGED_FILES=(
  /etc/vapor-server/ssh-access-enabled
  /etc/sudoers.d/vapor-temporary-access
  /usr/local/sbin/vapor-ssh-deploy
  /usr/local/sbin/vapor-ssh-service-control
)
for path in "${MANAGED_FILES[@]}"; do
  if [ -e "${path}" ] && { [ ! -f "${path}" ] || [ -L "${path}" ]; }; then
    echo "error: unexpected managed path ${path}; preserving it" >&2
    exit 1
  fi
done
EXPIRY_UNITS=()
for unit in /etc/systemd/system/vapor-ssh-expire-*.timer /etc/systemd/system/vapor-ssh-expire-*.service; do
  [ -e "${unit}" ] || continue
  [ -f "${unit}" ] && [ ! -L "${unit}" ] || {
    echo "error: unexpected expiry unit ${unit}; preserving it" >&2
    exit 1
  }
  if systemctl is-active --quiet "$(basename -- "${unit}")"; then
    echo "error: expiry unit is active; refusing to remove temporary SSH support" >&2
    exit 1
  fi
  EXPIRY_UNITS+=("${unit}")
done
if getent group vapor-temp-ssh >/dev/null; then
  if ! getent group vapor-temp-ssh | awk -F: '{ exit ($4 == "") ? 0 : 1 }'; then
    echo "error: vapor-temp-ssh still has members; group preserved" >&2
    exit 1
  fi
fi

CONFIG=/etc/ssh/sshd_config
if [ ! -f "${CONFIG}" ] || [ -L "${CONFIG}" ]; then
  echo "error: expected a regular OpenSSH server config; refusing to alter SSH" >&2
  exit 1
fi
begin='# BEGIN VAPOR TEMPORARY ACCESS'
end='# END VAPOR TEMPORARY ACCESS'
begin_count="$(grep -Fxc "${begin}" "${CONFIG}" || true)"
end_count="$(grep -Fxc "${end}" "${CONFIG}" || true)"
if [ "${begin_count}" != "${end_count}" ] || [ "${begin_count}" -gt 1 ]; then
  echo "error: temporary-access SSH markers are inconsistent; refusing to edit SSH" >&2
  exit 1
fi

if [ "${begin_count}" = 1 ]; then
  if systemctl is-active --quiet ssh.service; then
    SSH_SERVICE=ssh.service
  elif systemctl is-active --quiet sshd.service; then
    SSH_SERVICE=sshd.service
  else
    echo "error: no active SSH server; keeping temporary-access files intact" >&2
    exit 1
  fi
  backup="$(mktemp /etc/ssh/sshd_config.vapor-remove-backup.XXXXXX)"
  candidate="$(mktemp /etc/ssh/sshd_config.vapor-remove.XXXXXX)"
  cp -a -- "${CONFIG}" "${backup}"
  awk -v begin="${begin}" -v end="${end}" '
    $0 == begin { skip=1; next }
    $0 == end { skip=0; next }
    !skip { print }
  ' "${CONFIG}" > "${candidate}"
  chown --reference="${CONFIG}" "${candidate}"
  chmod --reference="${CONFIG}" "${candidate}"
  if ! sshd -t -f "${candidate}"; then
    rm -f -- "${backup}" "${candidate}"
    echo "error: OpenSSH rejected the cleanup config; no SSH change made" >&2
    exit 1
  fi
  mv -f -- "${candidate}" "${CONFIG}"
  if ! sshd -t || ! systemctl reload "${SSH_SERVICE}"; then
    cp -a -- "${backup}" "${CONFIG}"
    systemctl reload "${SSH_SERVICE}" || true
    rm -f -- "${backup}"
    echo "error: cleanup config failed; original SSH config restored" >&2
    exit 1
  fi
  rm -f -- "${backup}"
fi

for unit in "${EXPIRY_UNITS[@]}"; do
  unit_name="$(basename -- "${unit}")"
  if [[ "${unit_name}" == *.timer ]]; then
    if systemctl is-active --quiet "${unit_name}"; then systemctl stop "${unit_name}"; fi
    if systemctl is-enabled --quiet "${unit_name}"; then systemctl disable "${unit_name}"; fi
  fi
done
rm -f -- "${MANAGED_FILES[@]}"
visudo -c >/dev/null
rm -f -- "${EXPIRY_UNITS[@]}"
if getent group vapor-temp-ssh >/dev/null; then groupdel vapor-temp-ssh; fi
systemctl daemon-reload
echo "temporary SSH access removed; the shared Platform operation helper remains installed"
