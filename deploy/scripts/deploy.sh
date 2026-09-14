#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "${SCRIPT_DIR}/lib.sh"

require_root

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

# Browser identity has exactly one canonical public origin. This deliberately
# updates only non-secret settings; provider credentials remain untouched.
"${VAPOR_DEPLOY_ROOT}/deploy/scripts/configure-identity-auth.sh" \
  --public-origin "https://${VAPOR_DOMAIN}" \
  --cookie-secure true \
  --cookie-path /

cargo build --release --locked \
  --manifest-path "${VAPOR_DEPLOY_ROOT}/Vapor-Homepage-Server/Cargo.toml" \
  --target-dir "${VAPOR_DEPLOY_ROOT}/target"

cargo build --release --locked \
  --manifest-path "${VAPOR_DEPLOY_ROOT}/Vapor-Docs-Server/Cargo.toml" \
  --target-dir "${VAPOR_DEPLOY_ROOT}/target"

cargo build --release --locked \
  --manifest-path "${VAPOR_DEPLOY_ROOT}/Vapor-Identity-Server/Cargo.toml" \
  --target-dir "${VAPOR_DEPLOY_ROOT}/target"

cargo build --release --locked \
  --manifest-path "${VAPOR_DEPLOY_ROOT}/Vapor-Diagnostics-Server/Cargo.toml" \
  --target-dir "${VAPOR_DEPLOY_ROOT}/target"

cargo build --release --locked \
  --manifest-path "${VAPOR_DEPLOY_ROOT}/Vapor-Registry-Server/Cargo.toml" \
  --target-dir "${VAPOR_DEPLOY_ROOT}/target"

"${VAPOR_DEPLOY_ROOT}/deploy/scripts/install-systemd.sh"
"${VAPOR_DEPLOY_ROOT}/deploy/scripts/install-caddy.sh"
"${VAPOR_DEPLOY_ROOT}/deploy/scripts/install-auto-deploy.sh"
"${VAPOR_DEPLOY_ROOT}/deploy/scripts/install-state-backup.sh"

systemctl restart vapor-homepage.service
systemctl restart vapor-docs.service
systemctl restart vapor-identity.service
systemctl restart vapor-diagnostics.service
systemctl restart vapor-registry.service
systemctl restart caddy.service

"${VAPOR_DEPLOY_ROOT}/deploy/scripts/health-check.sh"

echo "deploy: completed ${VAPOR_REPO_URL}@${VAPOR_BRANCH}"