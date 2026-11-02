#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "${SCRIPT_DIR}/lib.sh"

require_root

case "${VAPOR_SERVER_MODE}" in
  monolith|services) ;;
  *)
    echo "error: VAPOR_SERVER_MODE must be 'monolith' or 'services'" >&2
    exit 1
    ;;
esac

DEPLOY_OPERATION_STARTED_AT_UTC="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
DEPLOY_OPERATION_REVISION="unknown"
record_deploy_operation() {
  local status="$?"
  local result="failed"
  local finished_at_utc
  trap - EXIT
  set +e
  if [ "$status" -eq 0 ]; then
    result="success"
  fi
  finished_at_utc="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  if ! write_operation_record \
    deploy "$result" "$DEPLOY_OPERATION_STARTED_AT_UTC" "$finished_at_utc" \
    "$DEPLOY_OPERATION_REVISION" "$VAPOR_SERVER_MODE"; then
    echo "warning: deploy result could not be recorded" >&2
  fi
  exit "$status"
}
trap record_deploy_operation EXIT

install -d -o root -g root -m 0755 "${VAPOR_DEPLOY_ROOT}"

if [ ! -d "${VAPOR_DEPLOY_ROOT}/.git" ]; then
  if [ -n "$(find "${VAPOR_DEPLOY_ROOT}" -mindepth 1 -maxdepth 1 -print -quit)" ]; then
    echo "error: ${VAPOR_DEPLOY_ROOT} is not empty and is not a git checkout" >&2
    exit 1
  fi

  git clone \
    --recurse-submodules \
    --branch "${VAPOR_BRANCH}" \
    "${VAPOR_REPO_URL}" \
    "${VAPOR_DEPLOY_ROOT}"
else
  git -C "${VAPOR_DEPLOY_ROOT}" fetch origin "${VAPOR_BRANCH}"
  git -C "${VAPOR_DEPLOY_ROOT}" checkout "${VAPOR_BRANCH}"
  git -C "${VAPOR_DEPLOY_ROOT}" reset --hard "origin/${VAPOR_BRANCH}"
  git -C "${VAPOR_DEPLOY_ROOT}" submodule sync --recursive
  git -C "${VAPOR_DEPLOY_ROOT}" submodule update --init --recursive
fi

DEPLOY_OPERATION_REVISION="$(git -C "${VAPOR_DEPLOY_ROOT}" rev-parse HEAD 2>/dev/null || printf 'unknown')"

# Atlas is a separately owned static site. Build it to its own public tree;
# never send its bundle through the Docs service's replacement endpoint.
"${VAPOR_DEPLOY_ROOT}/deploy/scripts/deploy-atlas.sh"

# Existing installations did not have Registry state/config. Create them
# idempotently here as well as in bootstrap so adding Registry does not require
# manual VPS migration before the normal deployment workflow can succeed.
install_dir \
  "${VAPOR_USER}" \
  "${VAPOR_GROUP}" \
  0750 \
  "${VAPOR_STATE_ROOT}/registry"

install_secret_env \
  "${VAPOR_CONFIG_DIR}/registry.env" \
"VAPOR_REGISTRY_BIND=127.0.0.1:7115
VAPOR_REGISTRY_STATE=${VAPOR_STATE_ROOT}/registry
VAPOR_REGISTRY_DB=${VAPOR_STATE_ROOT}/registry/registry.sqlite3"

install_dir "${VAPOR_USER}" "${VAPOR_GROUP}" 0750 "${VAPOR_STATE_ROOT}/updates"
install_dir "${VAPOR_USER}" "${VAPOR_GROUP}" 0750 "${VAPOR_STATE_ROOT}/build-cache"

# Docs publication depends on a server-local token. Bootstrap creates this on a
# fresh host; recreate it if an existing deployment lost the config file.
install_dir root "${VAPOR_GROUP}" 0750 "${VAPOR_CONFIG_DIR}"
install_secret_env \
  "${VAPOR_CONFIG_DIR}/artifact.env" \
"VAPOR_ARTIFACT_BIND=127.0.0.1:7116
VAPOR_UPDATES_ROOT=${VAPOR_STATE_ROOT}/updates"
install_secret_env \
  "${VAPOR_CONFIG_DIR}/build-cache.env" \
"VAPOR_BUILD_CACHE_BIND=127.0.0.1:7117
VAPOR_BUILD_CACHE_ROOT=${VAPOR_STATE_ROOT}/build-cache"
ensure_env_assignment \
  "${VAPOR_CONFIG_DIR}/build-cache.env" \
  VAPOR_BUILD_CACHE_BIND \
  127.0.0.1:7117
ensure_env_assignment \
  "${VAPOR_CONFIG_DIR}/build-cache.env" \
  VAPOR_BUILD_CACHE_ROOT \
  "${VAPOR_STATE_ROOT}/build-cache"
ensure_env_assignment \
  "${VAPOR_CONFIG_DIR}/platform.env" \
  VAPOR_PLATFORM_OPERATIONS_FILE \
  "${VAPOR_STATE_ROOT}/operations/latest.json"
install_dir "${VAPOR_USER}" "${VAPOR_GROUP}" 0750 "${VAPOR_STATE_ROOT}/docs"
if [ ! -e "${VAPOR_CONFIG_DIR}/docs.env" ]; then
  install_secret_env \
    "${VAPOR_CONFIG_DIR}/docs.env" \
"VAPOR_DOCS_BIND=127.0.0.1:7112
VAPOR_DOCS_STATE=${VAPOR_STATE_ROOT}/docs
VAPOR_DOCS_ADMIN_TOKEN=$(random_token)"
fi

# Browser identity has exactly one canonical public origin. This deliberately
# updates only non-secret settings; provider credentials remain untouched.
"${VAPOR_DEPLOY_ROOT}/deploy/scripts/configure-identity-auth.sh" \
  --public-origin "https://${VAPOR_DOMAIN}" \
  --cookie-secure true \
  --cookie-path /

bash "${VAPOR_DEPLOY_ROOT}/deploy/scripts/configure-authorization.sh"

if [ "${VAPOR_SERVER_MODE}" = "monolith" ]; then
  if [ ! -f "${VAPOR_DEPLOY_ROOT}/Cargo.lock" ]; then
    echo "error: composed server Cargo.lock is missing; generate and commit it before deploying monolith mode" >&2
    exit 1
  fi
  export VAPOR_PLATFORM_REVISION="$(git -C "${VAPOR_DEPLOY_ROOT}" rev-parse HEAD)"
  cargo build --release --locked \
    --manifest-path "${VAPOR_DEPLOY_ROOT}/Cargo.toml" \
    --target-dir "${VAPOR_DEPLOY_ROOT}/target"
  "${VAPOR_DEPLOY_ROOT}/deploy/scripts/install-systemd.sh"
  "${VAPOR_DEPLOY_ROOT}/deploy/scripts/install-caddy.sh"
else
  for service in homepage docs identity diagnostics registry artifact build-cache; do
    case "$service" in
      build-cache) service_package="Vapor-Build-Cache-Server" ;;
      *) service_package="Vapor-${service^}-Server" ;;
    esac
    cargo build --release --locked \
      --manifest-path "${VAPOR_DEPLOY_ROOT}/${service_package}/Cargo.toml" \
      --target-dir "${VAPOR_DEPLOY_ROOT}/target"
  done
  "${VAPOR_DEPLOY_ROOT}/deploy/scripts/install-services-systemd.sh"
  "${VAPOR_DEPLOY_ROOT}/deploy/scripts/install-services-caddy.sh"
fi

"${VAPOR_DEPLOY_ROOT}/deploy/scripts/install-auto-deploy.sh"
"${VAPOR_DEPLOY_ROOT}/deploy/scripts/install-state-backup.sh"

if [ "${VAPOR_SERVER_MODE}" = "monolith" ]; then
  systemctl restart vapor-platform-server.service
  wait_for_http \
    "Vapor Platform Server pre-cutover" \
    vapor-platform-server.service \
    http://127.0.0.1:7110/healthz
else
  systemctl restart vapor-homepage.service
  systemctl restart vapor-docs.service
  systemctl restart vapor-identity.service
  systemctl restart vapor-diagnostics.service
  systemctl restart vapor-registry.service
  systemctl restart vapor-artifact.service
  systemctl restart vapor-build-cache.service
fi
systemctl restart caddy.service

if [ "${VAPOR_SERVER_MODE}" = "monolith" ]; then
  for service in homepage docs identity diagnostics registry artifact build-cache; do
    systemctl disable --now "vapor-${service}.service" >/dev/null 2>&1 || true
  done
else
  systemctl disable --now vapor-platform-server.service >/dev/null 2>&1 || true
fi

"${VAPOR_DEPLOY_ROOT}/deploy/scripts/health-check.sh"

echo "deploy: completed ${VAPOR_REPO_URL}@${VAPOR_BRANCH}"
