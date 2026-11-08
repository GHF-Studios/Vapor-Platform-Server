#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "${SCRIPT_DIR}/lib.sh"
require_root

if ! getent group caddy >/dev/null; then
  echo "error: expected the Caddy service group; install Caddy first" >&2
  exit 1
fi

if [ ! -x /usr/bin/ttyd ]; then
  echo "error: install a reviewed ttyd build at /usr/bin/ttyd before preparing the emergency console" >&2
  exit 1
fi

# No upstream release is currently approved for this root-shell service:
# latest is 1.7.7, with an unresolved empty-WebSocket-frame crash report.
# Add an exact version only after reviewing a fixed upstream release and
# recording that review in the emergency-console design document.
APPROVED_TTYD_VERSIONS=()
TTYD_VERSION="unknown"
if TTYD_VERSION_OUTPUT="$(/usr/bin/ttyd --version 2>&1)"; then
  if [[ "${TTYD_VERSION_OUTPUT}" =~ ^ttyd\ version\ ([0-9]+\.[0-9]+\.[0-9]+)$ ]]; then
    TTYD_VERSION="${BASH_REMATCH[1]}"
  fi
fi
TTYD_VERSION_APPROVED=false
for approved_version in "${APPROVED_TTYD_VERSIONS[@]}"; do
  if [ "${TTYD_VERSION}" = "${approved_version}" ]; then
    TTYD_VERSION_APPROVED=true
    break
  fi
done
if [ "${TTYD_VERSION_APPROVED}" != true ]; then
  echo "error: ttyd ${TTYD_VERSION:-unknown} is not approved for a root console; no upstream ttyd release is currently approved" >&2
  exit 1
fi

install -d -o root -g root -m 0755 /usr/local/libexec
install -o root -g root -m 0750 \
  "${SCRIPT_DIR}/vapor-emergency-console-shell" \
  /usr/local/libexec/vapor-emergency-console-shell
install -o root -g root -m 0644 \
  "${SCRIPT_DIR}/vapor-emergency-console.bashrc" \
  /usr/local/libexec/vapor-emergency-console.bashrc
install -o root -g root -m 0644 \
  "${DEPLOY_DIR}/systemd/vapor-emergency-console.service" \
  /etc/systemd/system/vapor-emergency-console.service
systemctl daemon-reload

echo "emergency-console: prepared; not enabled at boot or started"
echo "emergency-console: using reviewed ttyd ${TTYD_VERSION}"
echo "emergency-console: root-owned platform.env must set VAPOR_EMERGENCY_CONSOLE_ENABLED=1 before the admin page can enable a session"
echo "emergency-console: update the Caddy config from the matching Platform release before use"
