#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"

CARGO="${CARGO:-cargo}"
TARGET_DIR="${VAPOR_PLATFORM_SERVER_TARGET_DIR:-${REPO_ROOT}/target}"

PACKAGES=(
  .
  Vapor-Homepage-Server
  Vapor-Docs-Server
  Vapor-Identity-Server
  Vapor-Diagnostics-Server
  Vapor-Registry-Server
  Vapor-Artifact-Server
  Vapor-Build-Cache-Server
)

echo "test: shell syntax"
bash -n "${SCRIPT_DIR}"/*.sh

echo "test: Python helper syntax"
python3 -m py_compile \
  "${SCRIPT_DIR}/state-bundle-validate.py" \
  "${SCRIPT_DIR}/recovery_bundle_metadata.py" \
  "${SCRIPT_DIR}/plan_recovery_retention.py" \
  "${SCRIPT_DIR}/restore_host_preflight.py" \
  "${SCRIPT_DIR}/deploy-request-queue.py" \
  "${SCRIPT_DIR}/verify-deploy-source.py" \
  "${SCRIPT_DIR}/service_deploy_map.py" \
  "${SCRIPT_DIR}/deployed_source_manifest.py" \
  "${SCRIPT_DIR}/operation_history.py" \
  "${SCRIPT_DIR}/render-caddyfile.py" \
  "${SCRIPT_DIR}/deploy-service.py"
rm -rf -- "${SCRIPT_DIR}/__pycache__"

echo "test: Atlas source and static site build"
python3 "${REPO_ROOT}/Atlas/tools/atlas.py" validate --root "${REPO_ROOT}/Atlas"
ATLAS_TEST_OUTPUT="$(mktemp -d)"
trap 'rm -rf -- "${ATLAS_TEST_OUTPUT}"' EXIT
python3 "${REPO_ROOT}/Atlas/tools/atlas.py" build \
  --root "${REPO_ROOT}/Atlas" \
  --output "${ATLAS_TEST_OUTPUT}" \
  --base-url /atlas/ \
  --source-revision test
test -s "${ATLAS_TEST_OUTPUT}/index.html"
test -s "${ATLAS_TEST_OUTPUT}/atlas.json"
rm -rf -- "${ATLAS_TEST_OUTPUT}"
trap - EXIT

echo "test: state-bundle validator"
python3 -m unittest discover   -s "${REPO_ROOT}/deploy/tests"   -p 'test_*.py'

for package in "${PACKAGES[@]}"; do
  echo "test: ${package}"

  "${CARGO}" test \
    --locked \
    --manifest-path "${REPO_ROOT}/${package}/Cargo.toml" \
    --target-dir "${TARGET_DIR}"
done

echo "test: all Vapor Platform Server checks passed"
