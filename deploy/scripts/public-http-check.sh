#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "${SCRIPT_DIR}/lib.sh"
# shellcheck source=http-contract-checks.sh
source "${SCRIPT_DIR}/http-contract-checks.sh"

if [ -n "${VAPOR_PUBLIC_HTTP_BASE:-}" ]; then
  base="${VAPOR_PUBLIC_HTTP_BASE%/}"
elif [ -n "${VAPOR_HTTP_FALLBACK_HOST}" ]; then
  base="http://${VAPOR_HTTP_FALLBACK_HOST}"
else
  echo "error: set VAPOR_PUBLIC_HTTP_BASE or VAPOR_HTTP_FALLBACK_HOST" >&2
  exit 1
fi

check_unauthenticated_http_contracts \
  --homepage-base "${base}" \
  --docs-base "${base}/docs" \
  --identity-base "${base}/api/identity" \
  --diagnostics-base "${base}/api/diagnostics"
curl --fail --silent --show-error "${base}/atlas/" >/dev/null
curl --fail --silent --show-error "${base}/login" >/dev/null
curl --fail --silent --show-error "${base}/admin" >/dev/null

echo "public-http: health/status checks passed for ${base}"
