#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
DEPLOY_DIR="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
REPO_ROOT="$(cd -- "${DEPLOY_DIR}/.." && pwd)"

VAPOR_DOMAIN="${VAPOR_DOMAIN:-vapor.ghf-studios.site}"
VAPOR_REPO_URL="${VAPOR_REPO_URL:-https://github.com/GHF-Studios/Vapor-Platform-Server.git}"
VAPOR_BRANCH="${VAPOR_BRANCH:-main}"
VAPOR_SERVER_MODE="${VAPOR_SERVER_MODE:-monolith}"
VAPOR_DEPLOY_ROOT="${VAPOR_DEPLOY_ROOT:-/opt/vapor-platform-server}"
VAPOR_STATE_ROOT="${VAPOR_STATE_ROOT:-/var/lib/vapor-server}"
VAPOR_CONFIG_DIR="${VAPOR_CONFIG_DIR:-/etc/vapor-server}"
VAPOR_ATLAS_WEB_ROOT="${VAPOR_ATLAS_WEB_ROOT:-/srv/vapor-web/atlas}"
if [ -r "${VAPOR_CONFIG_DIR}/platform.env" ]; then
  set -a
  # shellcheck disable=SC1090
  source "${VAPOR_CONFIG_DIR}/platform.env"
  set +a
fi

VAPOR_USER="${VAPOR_USER:-vapor}"
VAPOR_GROUP="${VAPOR_GROUP:-vapor}"
VAPOR_HTTP_FALLBACK_HOST="${VAPOR_HTTP_FALLBACK_HOST:-}"
VAPOR_TOOLCHAIN_ROOT="${VAPOR_TOOLCHAIN_ROOT:-/opt/vapor/tools}"
VAPOR_CARGO_HOME="${VAPOR_CARGO_HOME:-${VAPOR_TOOLCHAIN_ROOT}/cargo}"
VAPOR_RUSTUP_HOME="${VAPOR_RUSTUP_HOME:-${VAPOR_TOOLCHAIN_ROOT}/rustup}"
if [ -x "${VAPOR_CARGO_HOME}/bin/cargo" ]; then
  export CARGO_HOME="${VAPOR_CARGO_HOME}"
  export RUSTUP_HOME="${VAPOR_RUSTUP_HOME}"
  export PATH="${VAPOR_CARGO_HOME}/bin:${PATH}"
fi

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

ensure_env_assignment() {
  local path="$1"
  local key="$2"
  local value="$3"
  local directory
  local temporary

  if [[ ! "$key" =~ ^[A-Z][A-Z0-9_]*$ || "$value" == *$'\n'* || "$value" == *$'\r'* ]]; then
    echo "error: invalid environment assignment" >&2
    return 1
  fi
  if [ -L "$path" ]; then
    echo "error: refusing to update a symlinked environment file: $path" >&2
    return 1
  fi

  directory="$(dirname -- "$path")"
  install -d -o root -g "$VAPOR_GROUP" -m 0750 "$directory"
  temporary="$(mktemp "${path}.XXXXXX")"
  if [ -f "$path" ]; then
    awk -v key="${key}=" 'index($0, key) != 1 { print }' "$path" > "$temporary"
  fi
  printf '%s=%s\n' "$key" "$value" >> "$temporary"
  chown "root:${VAPOR_GROUP}" "$temporary"
  chmod 0640 "$temporary"
  mv -f -- "$temporary" "$path"
}

write_operation_record() {
  local operation="$1"
  local result="$2"
  local started_at_utc="$3"
  local finished_at_utc="$4"
  local revision="$5"
  local server_mode="$6"
  local directory="${VAPOR_STATE_ROOT}/operations"
  local temporary
  local timestamp_pattern='^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$'

  case "$operation" in deploy|restore) ;; *) return 1 ;; esac
  case "$result" in success|failed) ;; *) return 1 ;; esac
  case "$server_mode" in monolith|services) ;; *) return 1 ;; esac
  [[ "$started_at_utc" =~ $timestamp_pattern && "$finished_at_utc" =~ $timestamp_pattern ]] || return 1
  if [ "$revision" != "unknown" ] && [[ ! "$revision" =~ ^[0-9A-Fa-f]{7,40}$ ]]; then
    return 1
  fi
  if [ -L "$directory" ]; then
    echo "warning: refusing to write deploy/recovery status through a symlink" >&2
    return 1
  fi

  install -d -o root -g "$VAPOR_GROUP" -m 0750 "$directory"
  temporary="$(mktemp "${directory}/latest.json.XXXXXX")"
  umask 027
  printf '{"schema_version":1,"operation":"%s","result":"%s","started_at_utc":"%s","finished_at_utc":"%s","revision":"%s","server_mode":"%s"}\n' \
    "$operation" "$result" "$started_at_utc" "$finished_at_utc" "$revision" "$server_mode" \
    > "$temporary"
  if ! chown "root:${VAPOR_GROUP}" "$temporary" || ! chmod 0640 "$temporary"; then
    rm -f -- "$temporary"
    return 1
  fi
  if ! mv -f -- "$temporary" "${directory}/latest.json"; then
    rm -f -- "$temporary"
    return 1
  fi
}

wait_for_http() {
  local label="$1"
  local unit="$2"
  local url="$3"
  local attempts="${VAPOR_HEALTH_CHECK_ATTEMPTS:-30}"
  local interval="${VAPOR_HEALTH_CHECK_INTERVAL_SECONDS:-1}"
  local attempt
  local -a curl_options=("${@:4}")

  for ((attempt = 1; attempt <= attempts; attempt++)); do
    if curl \
      --fail \
      --silent \
      --show-error \
      --connect-timeout 1 \
      --max-time 3 \
      "${curl_options[@]}" \
      "$url" \
      >/dev/null 2>&1; then
      return 0
    fi

    if ((attempt < attempts)); then
      sleep "$interval"
    fi
  done

  echo "error: ${label} did not become ready after ${attempts} attempts: ${url}" >&2
  if command -v systemctl >/dev/null 2>&1; then
    systemctl status --no-pager --full "$unit" >&2 || true
  fi
  if command -v journalctl >/dev/null 2>&1; then
    journalctl --unit "$unit" --since "5 minutes ago" --no-pager >&2 || true
  fi
  return 1
}
