#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
DEPLOY_DIR="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
REPO_ROOT="$(cd -- "${DEPLOY_DIR}/.." && pwd)"

VAPOR_DOMAIN="${VAPOR_DOMAIN:-vapor.ghf-studios.site}"
VAPOR_REPO_URL="${VAPOR_REPO_URL:-https://github.com/GHF-Studios/Vapor-Platform-Server.git}"
VAPOR_BRANCH="${VAPOR_BRANCH:-main}"
VAPOR_DEPLOY_ROOT="${VAPOR_DEPLOY_ROOT:-/opt/vapor-platform-server}"
VAPOR_STATE_ROOT="${VAPOR_STATE_ROOT:-/var/lib/vapor-server}"
VAPOR_CONFIG_DIR="${VAPOR_CONFIG_DIR:-/etc/vapor-server}"
if [ -r "${VAPOR_CONFIG_DIR}/platform.env" ]; then
  set -a
  # shellcheck disable=SC1090
  source "${VAPOR_CONFIG_DIR}/platform.env"
  set +a
fi

VAPOR_USER="${VAPOR_USER:-vapor}"
VAPOR_GROUP="${VAPOR_GROUP:-vapor}"
VAPOR_HTTP_FALLBACK_HOST="${VAPOR_HTTP_FALLBACK_HOST:-}"

require_root() {
  if [ "$(id -u)" -ne 0 ]; then
    echo "error: this script must run as root" >&2
    exit 1
  fi
}

random_token() {
  openssl rand -hex 32
}

install_secret_env() {
  local path="$1"
  local contents="$2"

  if [ -e "$path" ]; then
    chown "root:${VAPOR_GROUP}" "$path"
    chmod 0640 "$path"
    return
  fi

  umask 027
  printf '%s\n' "$contents" > "$path"
  chown "root:${VAPOR_GROUP}" "$path"
  chmod 0640 "$path"
}

install_dir() {
  local owner="$1"
  local group="$2"
  local mode="$3"
  local path="$4"

  install -d -o "$owner" -g "$group" -m "$mode" "$path"
}
