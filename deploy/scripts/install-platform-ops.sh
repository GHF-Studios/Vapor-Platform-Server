#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/lib.sh"
require_root

HELPER="${VAPOR_DEPLOY_ROOT}/target/release/vapor_platform_ops"
SSH_CONFIG=/etc/ssh/sshd_config
if [ ! -x "${HELPER}" ]; then
  echo "error: the release vapor_platform_ops binary is missing" >&2
  exit 1
fi
if [ ! -f "${SSH_CONFIG}" ] || [ -L "${SSH_CONFIG}" ]; then
  echo "error: expected a regular OpenSSH server configuration; refusing to modify SSH" >&2
  exit 1
fi
if systemctl is-active --quiet ssh.service; then
  SSH_SERVICE=ssh.service
elif systemctl is-active --quiet sshd.service; then
  SSH_SERVICE=sshd.service
else
  echo "error: no active SSH server was found; refusing to alter SSH configuration" >&2
  exit 1
fi

begin='# BEGIN VAPOR TEMPORARY ACCESS'
end='# END VAPOR TEMPORARY ACCESS'
begin_count="$(grep -Fxc "${begin}" "${SSH_CONFIG}" || true)"
end_count="$(grep -Fxc "${end}" "${SSH_CONFIG}" || true)"
if [ "${begin_count}" != "${end_count}" ] || [ "${begin_count}" -gt 1 ]; then
  echo "error: temporary-access SSH markers are inconsistent; refusing to edit SSH" >&2
  exit 1
fi

temporary_config="$(mktemp /etc/ssh/sshd_config.vapor.XXXXXX)"
backup_config="$(mktemp /etc/ssh/sshd_config.vapor-backup.XXXXXX)"
cleanup() { rm -f -- "${temporary_config}" "${backup_config}"; }
trap cleanup EXIT
cp -a -- "${SSH_CONFIG}" "${backup_config}"
awk -v begin="${begin}" -v end="${end}" '
  $0 == begin { skip=1; next }
  $0 == end { skip=0; next }
  !skip { print }
' "${SSH_CONFIG}" > "${temporary_config}"
cat >> "${temporary_config}" <<EOF

${begin}
Match Group vapor-temp-ssh
    AuthenticationMethods publickey
    PasswordAuthentication no
    KbdInteractiveAuthentication no
    AllowTcpForwarding no
    X11Forwarding no
    PermitTunnel no
    AllowAgentForwarding no
    PermitTTY yes
${end}
EOF
chown --reference="${SSH_CONFIG}" "${temporary_config}"
chmod --reference="${SSH_CONFIG}" "${temporary_config}"
if ! sshd -t -f "${temporary_config}"; then
  echo "error: OpenSSH rejected the temporary-access configuration; original left intact" >&2
  exit 1
fi

install -d -o root -g root -m 0755 /usr/local/libexec
install -o root -g root -m 0750 "${HELPER}" /usr/local/libexec/vapor-platform-ops
install -d -o root -g root -m 0755 /etc/sudoers.d
temporary_sudoers="$(mktemp /etc/sudoers.d/vapor-platform-ops.XXXXXX)"
trap 'rm -f -- "${temporary_sudoers}"; cleanup' EXIT
printf '%s\n' 'vapor ALL=(root) NOPASSWD: /usr/local/libexec/vapor-platform-ops ""' > "${temporary_sudoers}"
chown root:root "${temporary_sudoers}"
chmod 0440 "${temporary_sudoers}"
visudo -cf "${temporary_sudoers}"
mv -f -- "${temporary_sudoers}" /etc/sudoers.d/vapor-platform-ops

install -o root -g root -m 0750 "${DEPLOY_DIR}/scripts/vapor-ssh-deploy" /usr/local/sbin/vapor-ssh-deploy
install -o root -g root -m 0750 "${DEPLOY_DIR}/scripts/vapor-ssh-service-control" /usr/local/sbin/vapor-ssh-service-control

# The group is only the SSH match/primary group. Capability-specific commands
# are granted by each request's own sudoers file; a group-wide rule would let
# every temporary account exceed its approved capability set.
legacy_sudoers=/etc/sudoers.d/vapor-temporary-access
if [ -e "${legacy_sudoers}" ] || [ -L "${legacy_sudoers}" ]; then
  if [ ! -f "${legacy_sudoers}" ] || [ -L "${legacy_sudoers}" ] || \
    [ "$(wc -l < "${legacy_sudoers}")" -ne 1 ] || \
    ! grep -Fqx '%vapor-temp-ssh ALL=(root) NOPASSWD: /usr/local/sbin/vapor-ssh-deploy "", /usr/local/sbin/vapor-ssh-service-control' "${legacy_sudoers}"; then
    echo "error: unexpected legacy temporary-access sudoers file; preserving it" >&2
    exit 1
  fi
  rm -- "${legacy_sudoers}"
fi
if include_directives="$(grep -RInE '^[[:space:]]*(@include(dir)?|#include(dir)?)[[:space:]]+' \
  /etc/sudoers /etc/sudoers.d 2>/dev/null)"; then
  if ! printf '%s\n' "${include_directives}" | awk -F: '
    $1 != "/etc/sudoers" { bad=1; next }
    {
      directive=$3
      sub(/^[[:space:]]*/, "", directive)
      sub(/[[:space:]]*$/, "", directive)
      if (directive != "@includedir /etc/sudoers.d" && directive != "#includedir /etc/sudoers.d") bad=1
    }
    END { exit bad }
  '; then
    echo "error: unsupported sudoers include path; refusing to enable temporary access" >&2
    exit 1
  fi
else
  include_scan_status=$?
  if [ "${include_scan_status}" -ne 1 ]; then
    echo "error: could not verify sudoers include paths; refusing to enable temporary access" >&2
    exit 1
  fi
fi
if grep -RIlE '^[[:space:]]*[^#[:space:]].*vapor-temp-ssh' \
  /etc/sudoers /etc/sudoers.d >/dev/null 2>&1; then
  echo "error: unexpected group-wide temporary-access sudo rule remains; refusing to enable access" >&2
  exit 1
else
  sudoers_scan_status=$?
  if [ "${sudoers_scan_status}" -ne 1 ]; then
    echo "error: could not verify temporary-access sudo policy; refusing to enable access" >&2
    exit 1
  fi
fi
visudo -c >/dev/null

if ! getent group vapor-temp-ssh >/dev/null; then
  groupadd --system vapor-temp-ssh
fi

mv -f -- "${temporary_config}" "${SSH_CONFIG}"
if ! sshd -t; then
  cp -a -- "${backup_config}" "${SSH_CONFIG}"
  echo "error: installed OpenSSH config failed validation; original restored" >&2
  exit 1
fi
if ! systemctl reload "${SSH_SERVICE}"; then
  cp -a -- "${backup_config}" "${SSH_CONFIG}"
  systemctl reload "${SSH_SERVICE}" || true
  echo "error: SSH reload failed; prior configuration restored" >&2
  exit 1
fi

systemctl daemon-reload
install -d -o root -g root -m 0750 /etc/vapor-server
temporary_flag="$(mktemp /etc/vapor-server/ssh-access-enabled.XXXXXX)"
chown root:root "${temporary_flag}"
chmod 0644 "${temporary_flag}"
mv -f -- "${temporary_flag}" /etc/vapor-server/ssh-access-enabled
echo "platform-ops: fixed helper and temporary SSH access policy installed"
