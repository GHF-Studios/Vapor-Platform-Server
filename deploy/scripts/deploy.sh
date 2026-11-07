#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "${SCRIPT_DIR}/lib.sh"

require_root
umask 022

if [ "$#" -eq 1 ] && [ "$1" = "--bootstrap" ]; then
  if [ -e "${VAPOR_DEPLOY_ROOT}/.git" ] || [ -n "$(find "${VAPOR_DEPLOY_ROOT}" -mindepth 1 -maxdepth 1 -print -quit 2>/dev/null)" ]; then
    echo "error: --bootstrap is only for an empty first-install checkout; use an approved request for updates" >&2
    exit 2
  fi
  if [ "${VAPOR_BRANCH}" != "main" ] || [ "${VAPOR_REPO_URL}" != "https://github.com/GHF-Studios/Vapor-Platform-Server.git" ]; then
    echo "error: first-install bootstrap is restricted to the official main repository" >&2
    exit 2
  fi
  install -d -o root -g root -m 0755 "${VAPOR_DEPLOY_ROOT}"
  git clone --branch main "${VAPOR_REPO_URL}" "${VAPOR_DEPLOY_ROOT}"
  BOOTSTRAP_REVISION="$(git -C "${VAPOR_DEPLOY_ROOT}" rev-parse HEAD)"
  python3 "${VAPOR_DEPLOY_ROOT}/deploy/scripts/verify-deploy-source.py" \
    --root "${VAPOR_DEPLOY_ROOT}" --commit "${BOOTSTRAP_REVISION}" >/dev/null
  git -C "${VAPOR_DEPLOY_ROOT}" submodule update --init
  VAPOR_DEPLOY_BOOTSTRAP=1 exec "${VAPOR_DEPLOY_ROOT}/deploy/scripts/deploy.sh" --approved "${BOOTSTRAP_REVISION}"
fi

if [ "$#" -ne 2 ] || [ "$1" != "--approved" ] || [[ ! "$2" =~ ^[A-Fa-f0-9]{40}$ ]]; then
  echo "error: use --approved <full-commit>; --bootstrap is only for a new empty installation" >&2
  exit 2
fi
APPROVED_REVISION="$2"
if [ "${VAPOR_DEPLOY_BOOTSTRAP:-}" != "1" ]; then
  if [[ ! "${VAPOR_APPROVED_DEPLOY_ID:-}" =~ ^[0-9a-fA-F-]{36}$ ]] || [[ ! "${VAPOR_APPROVED_DEPLOY_VERSION:-}" =~ ^[1-9][0-9]*$ ]]; then
    echo "error: approved request context is missing; invoke deploy.sh through the request worker" >&2
    exit 2
  fi
  QUEUE_DB="${VAPOR_PLATFORM_DB:-${VAPOR_PLATFORM_STATE:-${VAPOR_STATE_ROOT}/platform}/platform.sqlite3}"
  python3 "${SCRIPT_DIR}/deploy-request-queue.py" --database "${QUEUE_DB}" verify \
    --id "${VAPOR_APPROVED_DEPLOY_ID}" \
    --version "${VAPOR_APPROVED_DEPLOY_VERSION}" \
    --commit "${APPROVED_REVISION}"
fi

case "${VAPOR_SERVER_MODE}" in
  monolith|services) ;;
  *)
    echo "error: VAPOR_SERVER_MODE must be 'monolith' or 'services'" >&2
    exit 1
    ;;
esac
ensure_env_assignment \
  "${VAPOR_CONFIG_DIR}/platform.env" \
  VAPOR_SERVER_MODE \
  "${VAPOR_SERVER_MODE}"

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

if [ ! -d "${VAPOR_DEPLOY_ROOT}/.git" ]; then
  echo "error: approved deployment checkout is missing; initialize it through the documented bootstrap procedure" >&2
  exit 1
fi
if [ -n "$(git -C "${VAPOR_DEPLOY_ROOT}" status --porcelain)" ]; then
  echo "error: deployment checkout has local changes" >&2
  exit 1
fi
DEPLOY_OPERATION_REVISION="$(git -C "${VAPOR_DEPLOY_ROOT}" rev-parse HEAD)"
if [ "${DEPLOY_OPERATION_REVISION}" != "${APPROVED_REVISION}" ]; then
  echo "error: checked out revision does not match the approved revision" >&2
  exit 1
fi

ensure_runtime_access() {
  local path
  for path in \
    "${VAPOR_DEPLOY_ROOT}" \
    "${VAPOR_DEPLOY_ROOT}/target" \
    "${VAPOR_DEPLOY_ROOT}/target/release"; do
    if [ ! -d "${path}" ] || [ -L "${path}" ]; then
      echo "error: expected a regular runtime directory at ${path}" >&2
      return 1
    fi
    chown "root:${VAPOR_GROUP}" "${path}"
    chmod 0750 "${path}"
  done

  for path in "$@"; do
    if [ ! -f "${path}" ] || [ -L "${path}" ]; then
      echo "error: expected a regular server executable at ${path}" >&2
      return 1
    fi
    chown "root:${VAPOR_GROUP}" "${path}"
    chmod 0750 "${path}"
  done
}

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
install_dir "${VAPOR_USER}" "${VAPOR_GROUP}" 0750 "${VAPOR_STATE_ROOT}/platform"

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
ensure_env_assignment \
  "${VAPOR_CONFIG_DIR}/platform.env" \
  VAPOR_PLATFORM_STATE \
  "${VAPOR_STATE_ROOT}/platform"
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

# The privileged operation helper is built from the Platform package even in
# services mode, so its lockfile is required for every deployment mode.
if [ ! -f "${VAPOR_DEPLOY_ROOT}/Cargo.lock" ]; then
  echo "error: Platform Cargo.lock is missing; commit it before building the operation helper" >&2
  exit 1
fi

# Platform's root-only operation helper is installed in both deployment modes.
# Build it explicitly so a clean services-mode install does not depend on a
# binary left behind by an earlier monolith build.
export VAPOR_PLATFORM_REVISION="$(git -C "${VAPOR_DEPLOY_ROOT}" rev-parse HEAD)"
cargo build --release --locked \
  --manifest-path "${VAPOR_DEPLOY_ROOT}/Cargo.toml" \
  --target-dir "${VAPOR_DEPLOY_ROOT}/target" \
  --bin vapor_platform_ops

if [ "${VAPOR_SERVER_MODE}" = "monolith" ]; then
  cargo build --release --locked \
    --manifest-path "${VAPOR_DEPLOY_ROOT}/Cargo.toml" \
    --target-dir "${VAPOR_DEPLOY_ROOT}/target"
  ensure_runtime_access "${VAPOR_DEPLOY_ROOT}/target/release/vapor_platform_server"
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
  cargo build --release --locked \
    --manifest-path "${VAPOR_DEPLOY_ROOT}/Cargo.toml" \
    --target-dir "${VAPOR_DEPLOY_ROOT}/target" \
    --bin vapor_platform_server
  ensure_runtime_access \
    "${VAPOR_DEPLOY_ROOT}/target/release/vapor_homepage_server" \
    "${VAPOR_DEPLOY_ROOT}/target/release/vapor_docs_server" \
    "${VAPOR_DEPLOY_ROOT}/target/release/vapor_identity_server" \
    "${VAPOR_DEPLOY_ROOT}/target/release/vapor_diagnostics_server" \
    "${VAPOR_DEPLOY_ROOT}/target/release/vapor_registry_server" \
    "${VAPOR_DEPLOY_ROOT}/target/release/vapor_artifact_server" \
    "${VAPOR_DEPLOY_ROOT}/target/release/vapor_build_cache_server" \
    "${VAPOR_DEPLOY_ROOT}/target/release/vapor_platform_server"
  "${VAPOR_DEPLOY_ROOT}/deploy/scripts/install-services-systemd.sh"
  "${VAPOR_DEPLOY_ROOT}/deploy/scripts/install-services-caddy.sh"
fi

"${VAPOR_DEPLOY_ROOT}/deploy/scripts/install-approved-operations.sh"
"${VAPOR_DEPLOY_ROOT}/deploy/scripts/install-platform-ops.sh"
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
  systemctl restart vapor-platform-server.service
fi
systemctl restart caddy.service

"${VAPOR_DEPLOY_ROOT}/deploy/scripts/health-check.sh"

retire_service_unit() {
  local unit="$1"
  local load_state
  load_state="$(systemctl show --property=LoadState --value "${unit}" 2>/dev/null || printf 'unavailable')"
  case "${load_state}" in
    loaded)
      systemctl disable --now "${unit}"
      ;;
    not-found)
      ;;
    *)
      echo "error: could not verify optional service unit ${unit} before retiring the previous server mode" >&2
      return 1
      ;;
  esac
}

if [ "${VAPOR_SERVER_MODE}" = "monolith" ]; then
  for service in homepage docs identity diagnostics registry artifact build-cache; do
    retire_service_unit "vapor-${service}.service"
  done
else
  retire_service_unit vapor-platform-server.service
fi

echo "deploy: completed ${VAPOR_REPO_URL}@${VAPOR_BRANCH}"
