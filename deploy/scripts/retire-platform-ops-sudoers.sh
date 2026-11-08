#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/lib.sh"
require_root

platform_policy=/etc/sudoers.d/vapor-platform-ops
temporary_policy=/etc/sudoers.d/vapor-temporary-access
expected_platform="$(mktemp /tmp/vapor-platform-sudoers.XXXXXX)"
expected_temporary="$(mktemp /tmp/vapor-temporary-sudoers.XXXXXX)"
cleanup() { rm -f -- "${expected_platform}" "${expected_temporary}"; }
trap cleanup EXIT

printf '%s\n' \
  'vapor ALL=(root) NOPASSWD: /usr/local/libexec/vapor-platform-ops ""' \
  'vapor ALL=(root) NOPASSWD: /usr/local/libexec/vapor-platform-ops recovery-status' \
  'vapor ALL=(root) NOPASSWD: /usr/local/libexec/vapor-platform-ops emergency-console-start' \
  'vapor ALL=(root) NOPASSWD: /usr/local/libexec/vapor-platform-ops emergency-console-stop' \
  > "${expected_platform}"
printf '%s\n' \
  '%vapor-temp-ssh ALL=(root) NOPASSWD: /usr/local/sbin/vapor-ssh-deploy "", /usr/local/sbin/vapor-ssh-service-control' \
  > "${expected_temporary}"

for path in "${platform_policy}" "${temporary_policy}"; do
  [ -e "${path}" ] || [ -L "${path}" ] || continue
  if [ ! -f "${path}" ] || [ -L "${path}" ]; then
    echo "error: unexpected legacy sudoers path; preserving it: ${path}" >&2
    exit 1
  fi
done
if [ -e "${platform_policy}" ] && ! cmp -s -- "${expected_platform}" "${platform_policy}"; then
  echo "error: Platform sudoers file differs from the exact retired policy; preserving it" >&2
  exit 1
fi
if [ -e "${temporary_policy}" ] && ! cmp -s -- "${expected_temporary}" "${temporary_policy}"; then
  echo "error: temporary-access sudoers file differs from the exact retired policy; preserving it" >&2
  exit 1
fi

# Both files are validated before either is removed. This runs only after the
# replacement socket is active and the new Platform process passes health-check.
visudo -c >/dev/null
rm -f -- "${platform_policy}" "${temporary_policy}"
visudo -c >/dev/null
echo "platform-ops: exact legacy sudo policies retired"
