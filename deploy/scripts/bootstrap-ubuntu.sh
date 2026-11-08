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

# Package maintainer scripts may start newly installed daemons. During a
# fresh-host recovery, keep those services stopped until the saved state and
# configuration have been restored by restore-state.sh. Never replace a host
# policy supplied by the operator or base image.
if [ "${VAPOR_RECOVERY_STAGING:-0}" = "1" ]; then
  recovery_policy=/usr/sbin/policy-rc.d
  recovery_policy_digest="${VAPOR_RECOVERY_BUNDLE_SHA256:-}"
  [[ "${recovery_policy_digest}" =~ ^[0-9a-f]{64}$ ]] || {
    echo "error: recovery staging requires the verified bundle digest" >&2
    exit 1
  }
  recovery_policy_marker="# VAPOR_RECOVERY_STAGING ${recovery_policy_digest}"
  expected_recovery_policy="$(printf '#!/bin/sh\n%s\nexit 101' "${recovery_policy_marker}")"
  recovery_policy_tmp=""
  if [ -e "${recovery_policy}" ] || [ -L "${recovery_policy}" ]; then
    if [ -L "${recovery_policy}" ] || [ ! -f "${recovery_policy}" ] \
      || [ "$(cat -- "${recovery_policy}")" != "${expected_recovery_policy}" ]; then
      echo "error: existing policy-rc.d prevents controlled recovery package setup" >&2
      exit 1
    fi
  else
    recovery_policy_tmp="$(mktemp /usr/sbin/.policy-rc.d.recovery.XXXXXX)"
    printf '%s\n' '#!/bin/sh' "${recovery_policy_marker}" 'exit 101' > "${recovery_policy_tmp}"
    chmod 0755 "${recovery_policy_tmp}"
    # Hard-link creation is atomic and fails if an operator policy appeared
    # after the earlier check; never replace it during recovery setup.
    if ! ln -- "${recovery_policy_tmp}" "${recovery_policy}"; then
      rm -f -- "${recovery_policy_tmp}"
      echo "error: policy-rc.d appeared during controlled recovery setup" >&2
      exit 1
    fi
    rm -f -- "${recovery_policy_tmp}"
  fi
  cleanup_recovery_policy() {
    if [ -n "${recovery_policy_tmp}" ]; then
      rm -f -- "${recovery_policy_tmp}"
    fi
    if [ -f "${recovery_policy}" ] && [ ! -L "${recovery_policy}" ] \
      && [ "$(cat -- "${recovery_policy}")" = "${expected_recovery_policy}" ]; then
      rm -f -- "${recovery_policy}"
    else
      echo "error: recovery package-start policy changed during bootstrap; leaving it in place for review" >&2
      return 1
    fi
  }
  trap cleanup_recovery_policy EXIT
fi

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

# Reject malformed host configuration before any app or Caddy configuration
# is written. Python is installed as a required Platform operator tool above.
validate_vapor_host_config

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

if [ "${VAPOR_RECOVERY_STAGING:-0}" = "1" ]; then
  systemctl disable --now caddy.service >/dev/null 2>&1 || true
  trap - EXIT
  cleanup_recovery_policy
fi

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
VAPOR_ADMIN_DOMAIN=${VAPOR_ADMIN_DOMAIN}
VAPOR_PLATFORM_BIND=127.0.0.1:7110
VAPOR_SERVER_MODE=${VAPOR_SERVER_MODE}
VAPOR_PLATFORM_ADMIN_ORIGIN=${VAPOR_PLATFORM_ADMIN_ORIGIN}
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
# Public accounts and root operators have distinct host-only browser sessions.
VAPOR_IDENTITY_PUBLIC_ORIGIN=https://${VAPOR_DOMAIN}
VAPOR_IDENTITY_OPERATOR_ORIGIN=${VAPOR_PLATFORM_ADMIN_ORIGIN}
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
