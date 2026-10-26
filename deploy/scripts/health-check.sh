#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "${SCRIPT_DIR}/lib.sh"
# shellcheck source=http-contract-checks.sh
source "${SCRIPT_DIR}/http-contract-checks.sh"

wait_for_http "Vapor Homepage" vapor-homepage.service http://127.0.0.1:7111/healthz

wait_for_http "Vapor Docs" vapor-docs.service http://127.0.0.1:7112/healthz
curl --fail --silent --show-error http://127.0.0.1:7112/status >/dev/null

wait_for_http "Vapor Identity" vapor-identity.service http://127.0.0.1:7113/healthz
curl --fail --silent --show-error http://127.0.0.1:7113/v1/auth/status >/dev/null

wait_for_http "Vapor Diagnostics" vapor-diagnostics.service http://127.0.0.1:7114/healthz
curl --fail --silent --show-error http://127.0.0.1:7114/status >/dev/null

# Verify the static Atlas site through the same local Caddy route clients use.
# Resolving the canonical host to loopback avoids depending on public DNS here.
wait_for_http \
  "Atlas through Caddy" \
  caddy.service \
  "https://${VAPOR_DOMAIN}/atlas/" \
  --resolve "${VAPOR_DOMAIN}:443:127.0.0.1"

wait_for_http "Vapor Registry" vapor-registry.service http://127.0.0.1:7115/healthz
curl --fail --silent --show-error \
  http://127.0.0.1:7115/v1/ecosystems/ghf-studios/vapor \
  >/dev/null

check_unauthenticated_http_contracts \
  --homepage-base http://127.0.0.1:7111 \
  --docs-base http://127.0.0.1:7112 \
  --identity-base http://127.0.0.1:7113 \
  --diagnostics-base http://127.0.0.1:7114

systemctl is-enabled --quiet vapor-state-export.timer
systemctl is-active --quiet vapor-state-export.timer

echo "health: all Vapor services responded locally"
