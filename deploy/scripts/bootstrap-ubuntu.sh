#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "${SCRIPT_DIR}/lib.sh"

require_root

if ! grep -q '^ID=ubuntu$' /etc/os-release; then
  echo "error: bootstrap-ubuntu.sh supports Ubuntu only" >&2
  exit 1
fi

export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install --yes \
  apt-transport-https \
  build-essential \
  ca-certificates \
  curl \
  debian-archive-keyring \
  debian-keyring \
  git \
  gnupg \
  libssl-dev \
  libsqlite3-dev \
  openssh-server \
  openssl \
  pkg-config \
  python3 \
  sqlite3

# Install Caddy from its official Debian/Ubuntu package repository.
curl --fail --silent --show-error --location \
  'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' \
  | gpg --dearmor --yes -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
curl --fail --silent --show-error --location \
  'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' \
  -o /etc/apt/sources.list.d/caddy-stable.list
chmod o+r /usr/share/keyrings/caddy-stable-archive-keyring.gpg /etc/apt/sources.list.d/caddy-stable.list
apt-get update
apt-get install --yes caddy

VAPOR_TOOLCHAIN_ROOT="${VAPOR_TOOLCHAIN_ROOT:-/opt/vapor/tools}"
VAPOR_CARGO_HOME="${VAPOR_CARGO_HOME:-${VAPOR_TOOLCHAIN_ROOT}/cargo}"
VAPOR_RUSTUP_HOME="${VAPOR_RUSTUP_HOME:-${VAPOR_TOOLCHAIN_ROOT}/rustup}"
install -d -o root -g root -m 0755 "${VAPOR_TOOLCHAIN_ROOT}" "${VAPOR_CARGO_HOME}" "${VAPOR_RUSTUP_HOME}"
if [ ! -x "${VAPOR_CARGO_HOME}/bin/cargo" ]; then
  rustup_installer="$(mktemp)"
  trap 'rm -f -- "${rustup_installer}"' EXIT
  curl --fail --silent --show-error --location 'https://sh.rustup.rs' -o "${rustup_installer}"
  chmod 0700 "${rustup_installer}"
  CARGO_HOME="${VAPOR_CARGO_HOME}" RUSTUP_HOME="${VAPOR_RUSTUP_HOME}" \
    sh "${rustup_installer}" --yes --profile minimal --default-toolchain stable
  rm -f -- "${rustup_installer}"
  trap - EXIT
fi

if ! getent group "${VAPOR_GROUP}" >/dev/null; then
  groupadd --system "${VAPOR_GROUP}"
fi
if ! id -u "${VAPOR_USER}" >/dev/null 2>&1; then
  useradd --system --gid "${VAPOR_GROUP}" --home-dir "${VAPOR_STATE_ROOT}" \
    --shell /usr/sbin/nologin --comment "Vapor Platform services" "${VAPOR_USER}"
fi

install_dir root root 0755 /opt
install_dir root root 0755 "${VAPOR_DEPLOY_ROOT}"

install_dir "${VAPOR_USER}" "${VAPOR_GROUP}" 0750 "${VAPOR_STATE_ROOT}"
install_dir "${VAPOR_USER}" "${VAPOR_GROUP}" 0750 "${VAPOR_STATE_ROOT}/homepage"
install_dir "${VAPOR_USER}" "${VAPOR_GROUP}" 0750 "${VAPOR_STATE_ROOT}/docs"
install_dir "${VAPOR_USER}" "${VAPOR_GROUP}" 0750 "${VAPOR_STATE_ROOT}/identity"
install_dir "${VAPOR_USER}" "${VAPOR_GROUP}" 0750 "${VAPOR_STATE_ROOT}/diagnostics"
install_dir "${VAPOR_USER}" "${VAPOR_GROUP}" 0750 "${VAPOR_STATE_ROOT}/registry"
install_dir "${VAPOR_USER}" "${VAPOR_GROUP}" 0750 "${VAPOR_STATE_ROOT}/updates"
install_dir "${VAPOR_USER}" "${VAPOR_GROUP}" 0750 "${VAPOR_STATE_ROOT}/build-cache"
install_dir "${VAPOR_USER}" "${VAPOR_GROUP}" 0750 "${VAPOR_STATE_ROOT}/platform"

install_dir root "${VAPOR_GROUP}" 0750 "${VAPOR_CONFIG_DIR}"

chown -R "${VAPOR_USER}:${VAPOR_GROUP}" "${VAPOR_STATE_ROOT}"
find "${VAPOR_STATE_ROOT}" -type d -exec chmod 0750 {} +
find "${VAPOR_STATE_ROOT}" -type f -exec chmod 0640 {} +

install_secret_env "${VAPOR_CONFIG_DIR}/platform.env" \
"VAPOR_DOMAIN=${VAPOR_DOMAIN}
VAPOR_PLATFORM_BIND=127.0.0.1:7110
VAPOR_SERVER_MODE=${VAPOR_SERVER_MODE}
VAPOR_PLATFORM_PROBE_ORIGIN=http://127.0.0.1:7110
VAPOR_PLATFORM_IDENTITY_AUTHZ_URL=http://127.0.0.1:7110/api/identity/v1/internal/authorization/check
VAPOR_BRANCH=${VAPOR_BRANCH}
VAPOR_TOOLCHAIN_ROOT=${VAPOR_TOOLCHAIN_ROOT}
VAPOR_CARGO_HOME=${VAPOR_CARGO_HOME}
VAPOR_RUSTUP_HOME=${VAPOR_RUSTUP_HOME}
# Optional pre-DNS HTTP test host, for example a server IP address:
# VAPOR_HTTP_FALLBACK_HOST="
ensure_env_assignment \
  "${VAPOR_CONFIG_DIR}/platform.env" \
  VAPOR_PLATFORM_OPERATIONS_FILE \
  "${VAPOR_STATE_ROOT}/operations/latest.json"
ensure_env_assignment \
  "${VAPOR_CONFIG_DIR}/platform.env" \
  VAPOR_PLATFORM_STATE \
  "${VAPOR_STATE_ROOT}/platform"
ensure_env_assignment \
  "${VAPOR_CONFIG_DIR}/platform.env" \
  VAPOR_TOOLCHAIN_ROOT \
  "${VAPOR_TOOLCHAIN_ROOT}"
ensure_env_assignment \
  "${VAPOR_CONFIG_DIR}/platform.env" \
  VAPOR_CARGO_HOME \
  "${VAPOR_CARGO_HOME}"
ensure_env_assignment \
  "${VAPOR_CONFIG_DIR}/platform.env" \
  VAPOR_RUSTUP_HOME \
  "${VAPOR_RUSTUP_HOME}"

install_secret_env "${VAPOR_CONFIG_DIR}/homepage.env" \
"VAPOR_HOMEPAGE_BIND=127.0.0.1:7111"

install_secret_env "${VAPOR_CONFIG_DIR}/docs.env" \
"VAPOR_DOCS_BIND=127.0.0.1:7112
VAPOR_DOCS_STATE=${VAPOR_STATE_ROOT}/docs
VAPOR_DOCS_ADMIN_TOKEN=$(random_token)"

install_secret_env "${VAPOR_CONFIG_DIR}/identity.env" \
"VAPOR_IDENTITY_BIND=127.0.0.1:7113
VAPOR_IDENTITY_STATE=${VAPOR_STATE_ROOT}/identity
VAPOR_IDENTITY_DB=${VAPOR_STATE_ROOT}/identity/identity.sqlite3
VAPOR_IDENTITY_ADMIN_TOKEN=$(random_token)
VAPOR_IDENTITY_DASHBOARD_PASSWORD=$(random_token)
VAPOR_IDENTITY_STEAM_APP_ID=2122620
VAPOR_IDENTITY_LOOCAST_APP_ID=2122620
VAPOR_IDENTITY_STEAM_AUTH_IDENTITY=vapor-identity
VAPOR_IDENTITY_COOKIE_PATH=/
VAPOR_IDENTITY_COOKIE_SECURE=true
# Browser identity is canonical on the HTTPS Vapor domain. Override manually
# only for an intentional non-production deployment.
VAPOR_IDENTITY_PUBLIC_ORIGIN=https://${VAPOR_DOMAIN}
# Set on the server when available; never commit the value:
VAPOR_IDENTITY_STEAM_WEB_API_KEY=
# Set after creating the GitHub OAuth/GitHub App registration:
VAPOR_IDENTITY_GITHUB_CLIENT_ID=
VAPOR_IDENTITY_GITHUB_CLIENT_SECRET="

install_secret_env "${VAPOR_CONFIG_DIR}/diagnostics.env" \
"VAPOR_DIAGNOSTICS_BIND=127.0.0.1:7114
VAPOR_DIAGNOSTICS_STATE=${VAPOR_STATE_ROOT}/diagnostics
VAPOR_DIAGNOSTICS_MAX_STORED_BYTES=268435456
VAPOR_DIAGNOSTICS_ADMIN_TOKEN=$(random_token)"

install_secret_env "${VAPOR_CONFIG_DIR}/registry.env" \
"VAPOR_REGISTRY_BIND=127.0.0.1:7115
VAPOR_REGISTRY_STATE=${VAPOR_STATE_ROOT}/registry
VAPOR_REGISTRY_DB=${VAPOR_STATE_ROOT}/registry/registry.sqlite3"

install_secret_env "${VAPOR_CONFIG_DIR}/artifact.env" \
"VAPOR_ARTIFACT_BIND=127.0.0.1:7116
VAPOR_UPDATES_ROOT=${VAPOR_STATE_ROOT}/updates"

install_secret_env "${VAPOR_CONFIG_DIR}/build-cache.env" \
"VAPOR_BUILD_CACHE_BIND=127.0.0.1:7117
VAPOR_BUILD_CACHE_ROOT=${VAPOR_STATE_ROOT}/build-cache"

bash "${SCRIPT_DIR}/configure-authorization.sh"
