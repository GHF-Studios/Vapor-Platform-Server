#!/usr/bin/env bash
set -Eeuo pipefail

report_failed_check() {
  local status="$?"
  local command="$BASH_COMMAND"
  local line="${BASH_LINENO[0]:-0}"
  local failure_file="${VAPOR_HEALTH_CHECK_FAILURE_FILE:-/var/tmp/vapor-health-check-failure.txt}"
  trap - ERR
  printf 'health-check: line %s failed (exit %s): %s\n' "$line" "$status" "$command" >&2
  (
    umask 077
    printf 'line=%s\nexit=%s\ncommand=%s\n' "$line" "$status" "$command" > "$failure_file"
  ) 2>/dev/null || true
  exit "$status"
}
trap report_failed_check ERR

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "${SCRIPT_DIR}/lib.sh"
# shellcheck source=http-contract-checks.sh
source "${SCRIPT_DIR}/http-contract-checks.sh"

if [ "${VAPOR_SERVER_MODE}" = "monolith" ]; then
  wait_for_http "Vapor Platform Server" vapor-platform-server.service http://127.0.0.1:7110/healthz
  curl --fail --silent --show-error http://127.0.0.1:7110/docs/status >/dev/null
  curl --fail --silent --show-error http://127.0.0.1:7110/api/identity/v1/auth/status >/dev/null
  curl --fail --silent --show-error http://127.0.0.1:7110/api/diagnostics/status >/dev/null
  curl --fail --silent --show-error \
    http://127.0.0.1:7110/api/registry/v1/ecosystems/ghf-studios/vapor \
    >/dev/null
  curl --fail --silent --show-error http://127.0.0.1:7110/api/build-cache/v1/status >/dev/null

  check_unauthenticated_http_contracts \
    --homepage-base http://127.0.0.1:7110 \
    --docs-base http://127.0.0.1:7110/docs \
    --identity-base http://127.0.0.1:7110/api/identity \
    --diagnostics-base http://127.0.0.1:7110/api/diagnostics
else
  wait_for_http "Vapor Homepage" vapor-homepage.service http://127.0.0.1:7111/healthz

  wait_for_http "Vapor Docs" vapor-docs.service http://127.0.0.1:7112/healthz
  curl --fail --silent --show-error http://127.0.0.1:7112/status >/dev/null

  wait_for_http "Vapor Identity" vapor-identity.service http://127.0.0.1:7113/healthz
  curl --fail --silent --show-error http://127.0.0.1:7113/v1/auth/status >/dev/null

  wait_for_http "Vapor Diagnostics" vapor-diagnostics.service http://127.0.0.1:7114/healthz
  curl --fail --silent --show-error http://127.0.0.1:7114/status >/dev/null

  wait_for_http "Vapor Registry" vapor-registry.service http://127.0.0.1:7115/healthz
  curl --fail --silent --show-error \
    http://127.0.0.1:7115/v1/ecosystems/ghf-studios/vapor \
    >/dev/null

  wait_for_http "Vapor Artifact" vapor-artifact.service http://127.0.0.1:7116/healthz
  wait_for_http "Vapor Build Cache" vapor-build-cache.service http://127.0.0.1:7117/healthz
  curl --fail --silent --show-error http://127.0.0.1:7117/v1/status >/dev/null

  check_unauthenticated_http_contracts \
    --homepage-base http://127.0.0.1:7111 \
    --docs-base http://127.0.0.1:7112 \
    --identity-base http://127.0.0.1:7113 \
    --diagnostics-base http://127.0.0.1:7114
fi

# Verify the static Atlas site through the same local Caddy route clients use.
# Resolving the canonical host to loopback avoids depending on public DNS here.
wait_for_http \
  "Atlas through Caddy" \
  caddy.service \
  "https://${VAPOR_DOMAIN}/atlas/" \
  --resolve "${VAPOR_DOMAIN}:443:127.0.0.1"

systemctl is-enabled --quiet vapor-state-export.timer
systemctl is-active --quiet vapor-state-export.timer

echo "health: Vapor server endpoints responded locally (${VAPOR_SERVER_MODE} mode)"
