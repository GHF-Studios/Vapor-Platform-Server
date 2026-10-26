#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"

CARGO="${CARGO:-cargo}"
TARGET_DIR="${VAPOR_PLATFORM_SERVER_TARGET_DIR:-${REPO_ROOT}/target}"

SERVICES=(
  Vapor-Homepage-Server
  Vapor-Docs-Server
  Vapor-Identity-Server
  Vapor-Diagnostics-Server
  Vapor-Registry-Server
)

echo "test: shell syntax"
bash -n "${SCRIPT_DIR}"/*.sh

echo "test: Python helper syntax"
python3 -m py_compile "${SCRIPT_DIR}/state-bundle-validate.py"

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

for service in "${SERVICES[@]}"; do
  echo "test: ${service}"

  "${CARGO}" test     --locked     --manifest-path "${REPO_ROOT}/${service}/Cargo.toml"     --target-dir "${TARGET_DIR}"
done

echo "test: all Vapor Platform Server checks passed"
