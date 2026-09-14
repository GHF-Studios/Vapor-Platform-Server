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

for service in "${SERVICES[@]}"; do
  echo "build: ${service}"

  "${CARGO}" build     --locked     --manifest-path "${REPO_ROOT}/${service}/Cargo.toml"     --target-dir "${TARGET_DIR}"     "$@"
done

echo "build: all Vapor Platform Server services built"
