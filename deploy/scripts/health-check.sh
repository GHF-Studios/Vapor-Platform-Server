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

check_admin_gateway_route() {
  local path="$1"
  local host="${VAPOR_ADMIN_DOMAIN:-${VAPOR_DOMAIN}}"
  local status
  status="$(curl --silent --show-error --connect-timeout 1 --max-time 3 \
    --resolve "${host}:443:127.0.0.1" \
    --output /dev/null --write-out '%{http_code}' "https://${host}${path}")"
  case "${status}" in
    2??|3??|401|403) ;;
    *)
      echo "error: Platform admin gateway route ${path} returned HTTP ${status}" >&2
      return 1
      ;;
  esac
}

check_public_admin_isolation() {
  local status redirect
  status="$(curl --silent --show-error --connect-timeout 1 --max-time 3 \
    --resolve "${VAPOR_DOMAIN}:443:127.0.0.1" \
    --output /dev/null --write-out '%{http_code}' "https://${VAPOR_DOMAIN}/admin")"
  [ "${status}" = 308 ] || {
    echo "error: public admin entry returned HTTP ${status}, expected 308" >&2
    return 1
  }
  redirect="$(curl --silent --show-error --connect-timeout 1 --max-time 3 \
    --resolve "${VAPOR_DOMAIN}:443:127.0.0.1" \
    --output /dev/null --write-out '%{redirect_url}' "https://${VAPOR_DOMAIN}/admin")"
  [ "${redirect}" = "https://${VAPOR_ADMIN_DOMAIN}/admin" ] || {
    echo "error: public admin redirect does not target the configured admin host" >&2
    return 1
  }
  status="$(curl --silent --show-error --connect-timeout 1 --max-time 3 \
    --resolve "${VAPOR_DOMAIN}:443:127.0.0.1" \
    --output /dev/null --write-out '%{http_code}' "https://${VAPOR_DOMAIN}/login")"
  [ "${status}" = 200 ] || {
    echo "error: public account login returned HTTP ${status}, expected 200" >&2
    return 1
  }
  status="$(curl --silent --show-error --connect-timeout 1 --max-time 3 \
    --resolve "${VAPOR_DOMAIN}:443:127.0.0.1" \
    --output /dev/null --write-out '%{http_code}' "https://${VAPOR_DOMAIN}/logout")"
  [ "${status}" = 303 ] || {
    echo "error: public account logout returned HTTP ${status}, expected 303" >&2
    return 1
  }
  for path in /operator/login /cli/ssh/authorize; do
    status="$(curl --silent --show-error --connect-timeout 1 --max-time 3 \
      --resolve "${VAPOR_DOMAIN}:443:127.0.0.1" \
      --output /dev/null --write-out '%{http_code}' "https://${VAPOR_DOMAIN}${path}")"
    [ "${status}" = 308 ] || {
      echo "error: public browser route ${path} returned HTTP ${status}, expected 308" >&2
      return 1
    }
  done
  status="$(curl --silent --show-error --connect-timeout 1 --max-time 3 \
    --resolve "${VAPOR_ADMIN_DOMAIN}:443:127.0.0.1" \
    --output /dev/null --write-out '%{http_code}' "https://${VAPOR_ADMIN_DOMAIN}/operator/login")"
  [ "${status}" = 200 ] || {
    echo "error: root operator login returned HTTP ${status}, expected 200" >&2
    return 1
  }
  for path in \
    /api/admin/platform-healthz \
    /api/identity/v1/admin/profiles \
    /api/identity/v1/internal/authorization/check \
    /emergency-terminal/; do
    status="$(curl --silent --show-error --connect-timeout 1 --max-time 3 \
      --resolve "${VAPOR_DOMAIN}:443:127.0.0.1" \
      --output /dev/null --write-out '%{http_code}' "https://${VAPOR_DOMAIN}${path}")"
    [ "${status}" = 404 ] || {
      echo "error: public privileged route ${path} returned HTTP ${status}, expected 404" >&2
      return 1
    }
  done
}

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
  wait_for_http "Vapor Platform Admin Gateway" vapor-platform-server.service http://127.0.0.1:7110/healthz
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

  # Service health does not prove that Caddy's shared admin gateway can reach
  # each owner page. Check the fixed routes end-to-end through local Caddy after
  # all owner services are ready; login redirects and normal authorization
  # results are valid responses, while 404/5xx indicate a broken mapping.
  if [ -n "${VAPOR_ADMIN_DOMAIN}" ]; then
    check_admin_gateway_route /admin/identity
    check_admin_gateway_route /admin/registry
    check_admin_gateway_route /admin/artifacts
    check_admin_gateway_route /admin/build-cache
    check_public_admin_isolation
  else
    check_admin_gateway_route /admin/identity
    check_admin_gateway_route /admin/registry
    check_admin_gateway_route /admin/artifacts
    check_admin_gateway_route /admin/build-cache
  fi

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

if systemctl is-enabled --quiet vapor-state-export.timer || systemctl is-active --quiet vapor-state-export.timer; then
  echo "health: legacy full-host export timer must be disabled" >&2
  exit 1
fi
systemctl is-enabled --quiet vapor-state-only-export.timer
if [ "${VAPOR_RECOVERY_STAGING:-0}" != "1" ]; then
  systemctl is-active --quiet vapor-state-only-export.timer
fi

echo "health: Vapor server endpoints responded locally (${VAPOR_SERVER_MODE} mode)"
