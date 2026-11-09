#!/usr/bin/env bash
set -Eeuo pipefail
umask 077

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly REPOSITORY_URL="https://github.com/GHF-Studios/Vapor-Platform-Server.git"
readonly DEPLOY_ROOT="/opt/vapor-platform-server"
readonly STATE_ROOT="/var/lib/vapor-server"
readonly CONFIG_ROOT="/etc/vapor-server"
readonly TOOLCHAIN_ROOT="/opt/vapor/tools"
readonly MARKER_ROOT="/var/lib/vapor-state-recovery"
readonly MARKER_TOOL="${SCRIPT_DIR}/state_only_recovery_marker.py"

BUNDLE=""
CONFIRMED=false
RESUME=false
CONFIRM_PROVIDER_CHECKS=false
HOST_MUTATION_STARTED=false
RECOVERY_AWAITING_CONFIRMATION=false
STATUS_FILE=""

usage() {
  cat <<'USAGE'
usage: recover-state-only-host.sh --bundle /path/to/state-snapshot.tar.gz --yes [--resume]
       recover-state-only-host.sh --bundle /path/to/state-snapshot.tar.gz --yes --resume --confirm-provider-checks

Rebuilds a blank Ubuntu x86_64 Vapor host from source, re-enters external
provider credentials without echoing them, then imports the state-only archive.
The state-only marker makes interrupted setup resumable with the same archive.
After restore, sign in and verify the owner dashboard, then rerun with
--resume --confirm-provider-checks to close the recovery marker.
USAGE
}

fail() { echo "state-only-recovery: $*" >&2; exit 1; }

safe_resume_secret_file() {
  local path="$1"
  local owner group mode config_group
  [ -f "$path" ] && [ ! -L "$path" ] || return 1
  read -r owner group mode < <(stat -c '%u %g %a' "$path") || return 1
  [ "$owner" = "0" ] || return 1
  case "$mode" in
    600) return 0 ;;
    640)
      config_group="$(stat -c '%g' "$CONFIG_ROOT")" || return 1
      [ "$group" = "$config_group" ]
      ;;
    *) return 1 ;;
  esac
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --bundle) [ "$#" -ge 2 ] || { usage >&2; exit 2; }; BUNDLE="$2"; shift 2 ;;
    --yes) CONFIRMED=true; shift ;;
    --resume) RESUME=true; shift ;;
    --confirm-provider-checks) CONFIRM_PROVIDER_CHECKS=true; shift ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
  esac
done

[ "$(id -u)" -eq 0 ] || fail "run as root"
[ -n "$BUNDLE" ] && [ "$CONFIRMED" = true ] || { usage >&2; exit 2; }
[ "$CONFIRM_PROVIDER_CHECKS" = false ] || [ "$RESUME" = true ] \
  || fail "--confirm-provider-checks requires --resume"
[ -f "$BUNDLE" ] && [ ! -L "$BUNDLE" ] || fail "state snapshot must be a regular file"
[ -f "${BUNDLE}.sha256" ] && [ ! -L "${BUNDLE}.sha256" ] || fail "state snapshot checksum sidecar is missing"

# Validate the recovery kit and archive before any host mutation.
(cd -- "$SCRIPT_DIR" && sha256sum --check --status SHA256SUMS) \
  || fail "recovery kit files do not match their pinned checksums"
python3 "${SCRIPT_DIR}/recovery_bundle_metadata.py" --check-archive-checksum "$BUNDLE"
python3 "${SCRIPT_DIR}/state-bundle-validate.py" "$BUNDLE"
REVISION="$(python3 "${SCRIPT_DIR}/recovery_bundle_metadata.py" --source-commit "$BUNDLE")" \
  || fail "state snapshot has no valid source revision"
BUNDLE_SHA256="$(sha256sum "$BUNDLE" | awk '{print $1}')"

. /etc/os-release
[ "${ID:-}" = ubuntu ] || fail "only Ubuntu is supported"
[ "$(dpkg --print-architecture)" = amd64 ] || fail "only Ubuntu x86_64/amd64 is supported"
python3 -c 'import sys; raise SystemExit(0 if sys.version_info >= (3, 11) else 1)' \
  || fail "Python 3.11 or newer is required to validate this archive"

# Serialize host mutation only after the supplied archive and source pin have
# passed validation. The lock itself is volatile runtime coordination state.
[ -d /run/lock ] || fail "runtime lock directory is unavailable"
exec 9>/run/lock/vapor-state-recovery.lock
flock -n 9 || fail "another state-only recovery run is active"

marker_phase="building"
if [ "$RESUME" = true ]; then
  marker_json="$(python3 "$MARKER_TOOL" read --root "$MARKER_ROOT" \
    --bundle-sha256 "$BUNDLE_SHA256" --root-commit "$REVISION")" \
    || fail "recovery marker is missing, unsafe, or belongs to a different archive/source"
  marker_phase="$(python3 -c 'import json,sys; print(json.load(sys.stdin)["phase"])' <<<"$marker_json")"
  [ "$CONFIRM_PROVIDER_CHECKS" = false ] || [ "$marker_phase" = awaiting_provider_checks ] \
    || fail "provider-check confirmation is only valid after state restore"
elif [ -e "$MARKER_ROOT" ] || [ -L "$MARKER_ROOT" ]; then
  python3 "$MARKER_TOOL" prepare --root "$MARKER_ROOT" --bundle-sha256 "$BUNDLE_SHA256" \
    --root-commit "$REVISION" >/dev/null \
    || fail "unfinished or unexpected recovery marker exists; inspect it and use --resume"
fi

if [ "$RESUME" = true ]; then
  for path in "$DEPLOY_ROOT" "$STATE_ROOT" "$CONFIG_ROOT" "$TOOLCHAIN_ROOT" /etc/caddy/Caddyfile; do
    [ ! -L "$path" ] || fail "refusing symlink at ${path} during resume"
    if [ -e "$path" ] && [ "$path" != /etc/caddy/Caddyfile ] && [ ! -d "$path" ]; then
      fail "recovery path is not a directory: ${path}"
    fi
  done
fi

require_empty_or_absent() {
  local path="$1"
  [ ! -L "$path" ] || fail "refusing symlink at ${path}"
  if [ -e "$path" ] && { [ ! -d "$path" ] || [ -n "$(find "$path" -mindepth 1 -maxdepth 1 -print -quit)" ]; }; then
    fail "${path} is not empty; recovery cannot adopt unrelated host data"
  fi
}

if [ "$RESUME" = false ]; then
  require_empty_or_absent "$DEPLOY_ROOT"
  require_empty_or_absent "$STATE_ROOT"
  require_empty_or_absent "$CONFIG_ROOT"
  require_empty_or_absent "$TOOLCHAIN_ROOT"
  if [ -e /etc/caddy/Caddyfile ] || [ -L /etc/caddy/Caddyfile ]; then
    fail "existing Caddy configuration found; refusing to replace it"
  fi
fi

# Use the root-owned configuration created during an interrupted run when it
# exists. It is generated as shell-compatible assignments from constrained
# values; otherwise collect fresh provider credentials through hidden prompts.
if [ "$RESUME" = true ] && { [ -e "$CONFIG_ROOT/platform.env" ] || [ -L "$CONFIG_ROOT/platform.env" ]; }; then
  [ -f "$CONFIG_ROOT/platform.env" ] && [ ! -L "$CONFIG_ROOT/platform.env" ] \
    || fail "existing Platform configuration is not a regular file"
fi
if [ "$RESUME" = true ] && [ -f "$CONFIG_ROOT/platform.env" ] && [ ! -L "$CONFIG_ROOT/platform.env" ]; then
  [ -d "$CONFIG_ROOT" ] && [ ! -L "$CONFIG_ROOT" ] \
    && [ "$(stat -c '%u:%a' "$CONFIG_ROOT")" = "0:750" ] \
    || fail "existing Platform configuration directory has unsafe ownership or permissions"
  safe_resume_secret_file "$CONFIG_ROOT/platform.env" \
    || fail "existing Platform configuration has unsafe ownership or permissions"
  set -a
  # shellcheck disable=SC1091
  . "$CONFIG_ROOT/platform.env"
  set +a
  DOMAIN="${VAPOR_DOMAIN:-}"
  ADMIN_DOMAIN="${VAPOR_ADMIN_DOMAIN:-$DOMAIN}"
  SERVER_MODE="${VAPOR_SERVER_MODE:-services}"
  if [ -e "$CONFIG_ROOT/identity.env" ] || [ -L "$CONFIG_ROOT/identity.env" ]; then
    [ -f "$CONFIG_ROOT/identity.env" ] && [ ! -L "$CONFIG_ROOT/identity.env" ] \
      || fail "existing Identity configuration is not a regular file"
    safe_resume_secret_file "$CONFIG_ROOT/identity.env" \
      || fail "existing Identity configuration has unsafe ownership or permissions"
    # shellcheck disable=SC1091
    . "$CONFIG_ROOT/identity.env"
    STEAM_KEY="${VAPOR_IDENTITY_STEAM_WEB_API_KEY:-}"
    GITHUB_CLIENT_ID="${VAPOR_IDENTITY_GITHUB_CLIENT_ID:-}"
    GITHUB_CLIENT_SECRET="${VAPOR_IDENTITY_GITHUB_CLIENT_SECRET:-}"
  else
    read -r -s -p 'Steam Web API key (hidden): ' STEAM_KEY
    printf '\n'
    read -r -p 'GitHub OAuth client ID (blank if not configured): ' GITHUB_CLIENT_ID
    GITHUB_CLIENT_SECRET=""
    if [ -n "$GITHUB_CLIENT_ID" ]; then
      read -r -s -p 'GitHub OAuth client secret (hidden): ' GITHUB_CLIENT_SECRET
      printf '\n'
    fi
  fi
else
  read -r -p 'Public domain [vapor.ghf-studios.site]: ' DOMAIN
  DOMAIN="${DOMAIN:-vapor.ghf-studios.site}"
  read -r -p "Admin domain [${DOMAIN}]: " ADMIN_DOMAIN
  ADMIN_DOMAIN="${ADMIN_DOMAIN:-$DOMAIN}"
  read -r -p 'Runtime mode [services]: ' SERVER_MODE
  SERVER_MODE="${SERVER_MODE:-services}"
  read -r -s -p 'Steam Web API key (hidden): ' STEAM_KEY
  printf '\n'
  read -r -p 'GitHub OAuth client ID (blank if not configured): ' GITHUB_CLIENT_ID
  GITHUB_CLIENT_SECRET=""
  if [ -n "$GITHUB_CLIENT_ID" ]; then
    read -r -s -p 'GitHub OAuth client secret (hidden): ' GITHUB_CLIENT_SECRET
    printf '\n'
  fi
fi

[ "$SERVER_MODE" = services ] || [ "$SERVER_MODE" = monolith ] || fail "runtime mode must be services or monolith"
[[ "$DOMAIN" =~ ^[A-Za-z0-9.-]+$ && "$ADMIN_DOMAIN" =~ ^[A-Za-z0-9.-]+$ ]] \
  || fail "domains contain unsupported characters"
[[ "$STEAM_KEY" =~ ^[A-Fa-f0-9]{32}$ ]] || fail "Steam Web API key must be 32 hexadecimal characters"
if [ -n "$GITHUB_CLIENT_ID" ]; then
  [[ "$GITHUB_CLIENT_ID" =~ ^[A-Za-z0-9._-]{1,128}$ ]] || fail "GitHub client ID contains unsupported characters"
  [[ "$GITHUB_CLIENT_SECRET" =~ ^[A-Za-z0-9._-]{1,256}$ ]] || fail "GitHub client secret is empty or contains unsupported characters"
fi

export VAPOR_DOMAIN="$DOMAIN"
export VAPOR_ADMIN_DOMAIN="$ADMIN_DOMAIN"
export VAPOR_SERVER_MODE="$SERVER_MODE"
export VAPOR_PLATFORM_ADMIN_ORIGIN="https://${ADMIN_DOMAIN}"
export VAPOR_REPO_URL="$REPOSITORY_URL"
export VAPOR_BRANCH=main
export VAPOR_DEPLOY_ROOT="$DEPLOY_ROOT"
export VAPOR_STATE_ROOT="$STATE_ROOT"
export VAPOR_CONFIG_DIR="$CONFIG_ROOT"
export VAPOR_TOOLCHAIN_ROOT="$TOOLCHAIN_ROOT"
export VAPOR_CARGO_HOME="${TOOLCHAIN_ROOT}/cargo"
export VAPOR_RUSTUP_HOME="${TOOLCHAIN_ROOT}/rustup"
export VAPOR_IDENTITY_STEAM_WEB_API_KEY="$STEAM_KEY"
export VAPOR_IDENTITY_GITHUB_CLIENT_ID="$GITHUB_CLIENT_ID"
export VAPOR_IDENTITY_GITHUB_CLIENT_SECRET="$GITHUB_CLIENT_SECRET"
unset STEAM_KEY GITHUB_CLIENT_ID GITHUB_CLIENT_SECRET

if [ "$RESUME" = false ]; then
  python3 "$MARKER_TOOL" init --root "$MARKER_ROOT" --bundle-sha256 "$BUNDLE_SHA256" \
    --root-commit "$REVISION" >/dev/null
  for path in "$DEPLOY_ROOT" "$STATE_ROOT" "$CONFIG_ROOT" "$TOOLCHAIN_ROOT"; do
    if [ -d "$path" ]; then
      rmdir -- "$path" || fail "${path} changed after preflight; refusing to remove it"
    fi
  done
fi

if [ "$marker_phase" = awaiting_provider_checks ]; then
  [ -f "$CONFIG_ROOT/platform.env" ] && [ ! -L "$CONFIG_ROOT/platform.env" ] \
    || fail "completed recovery configuration is missing"
  STATUS_URL="http://127.0.0.1:7113/v1/auth/status"
  if [ "$SERVER_MODE" = monolith ]; then
    STATUS_URL="http://127.0.0.1:7110/api/identity/v1/auth/status"
  fi
  STATUS_FILE="$(mktemp /run/vapor-state-recovery-status.XXXXXX)"
  trap 'rm -f -- "$STATUS_FILE"' EXIT
  curl --fail --silent --show-error --connect-timeout 3 --max-time 15 "$STATUS_URL" > "$STATUS_FILE" \
    || fail "Identity status endpoint did not respond after state restore"
  python3 "${SCRIPT_DIR}/recovery_bundle_metadata.py" --check-provider-readiness "$STATUS_FILE"
  if [ "$CONFIRM_PROVIDER_CHECKS" != true ]; then
    echo "state-only-recovery: provider readiness passed; complete browser sign-in and owner-dashboard checks, then rerun with --resume --confirm-provider-checks"
    exit 3
  fi
  python3 "$MARKER_TOOL" complete --root "$MARKER_ROOT" --bundle-sha256 "$BUNDLE_SHA256" --root-commit "$REVISION"
  echo "state-only-recovery: owner confirmed provider checks; recovery complete"
  exit 0
fi

TEMP_SOURCE="/opt/.vapor-state-recovery-${BUNDLE_SHA256:0:16}"
if [ -e "$DEPLOY_ROOT" ] || [ -L "$DEPLOY_ROOT" ]; then
  [ "$RESUME" = true ] && [ -d "$DEPLOY_ROOT/.git" ] && [ ! -L "$DEPLOY_ROOT" ] \
    || fail "deployment path exists outside this resumable recovery"
  SOURCE_ROOT="$DEPLOY_ROOT"
elif [ -e "$TEMP_SOURCE" ] || [ -L "$TEMP_SOURCE" ]; then
  [ "$RESUME" = true ] && [ -d "$TEMP_SOURCE/.git" ] && [ ! -L "$TEMP_SOURCE" ] \
    || fail "recovery checkout path exists and is not an owned resumable checkout"
  SOURCE_ROOT="$TEMP_SOURCE"
else
  SOURCE_ROOT="$TEMP_SOURCE"
fi

if [ -d "$SOURCE_ROOT/.git" ]; then
  [ "$(git -C "$SOURCE_ROOT" remote get-url origin)" = "$REPOSITORY_URL" ] \
    || fail "recovery checkout repository identity changed"
else
  install -d -o root -g root -m 0755 /opt
  git clone --no-checkout "$REPOSITORY_URL" "$SOURCE_ROOT"
fi
git -C "$SOURCE_ROOT" fetch --no-tags origin "$REVISION"
git -C "$SOURCE_ROOT" checkout --detach "$REVISION"
git -C "$SOURCE_ROOT" submodule sync --recursive
git -C "$SOURCE_ROOT" submodule update --init --recursive
python3 "$SOURCE_ROOT/deploy/scripts/verify-deploy-source.py" --root "$SOURCE_ROOT" --commit "$REVISION"
[ -z "$(git -C "$SOURCE_ROOT" status --porcelain --untracked-files=all)" ] \
  || fail "pinned recovery checkout contains local changes"

if [ "$SOURCE_ROOT" != "$DEPLOY_ROOT" ]; then
  [ ! -e "$DEPLOY_ROOT" ] && [ ! -L "$DEPLOY_ROOT" ] \
    || fail "deployment path appeared during recovery; refusing to replace it"
  mv -- "$SOURCE_ROOT" "$DEPLOY_ROOT"
fi

cleanup_failed_recovery() {
  local status=$?
  trap - EXIT
  [ -n "$STATUS_FILE" ] && rm -f -- "$STATUS_FILE"
  if [ "$status" -ne 0 ] && [ "$HOST_MUTATION_STARTED" = true ] \
    && [ "$RECOVERY_AWAITING_CONFIRMATION" = false ]; then
    for unit in vapor-homepage.service vapor-docs.service vapor-identity.service \
      vapor-diagnostics.service vapor-registry.service vapor-artifact.service \
      vapor-build-cache.service vapor-platform-server.service vapor-state-export.timer \
      vapor-state-only-export.timer caddy.service; do
      systemctl disable --now "$unit" >/dev/null 2>&1 || true
    done
    echo "state-only-recovery: partial services disabled; marker and checkout retained for --resume after review" >&2
  fi
  exit "$status"
}
trap cleanup_failed_recovery EXIT

export DEBIAN_FRONTEND=noninteractive
HOST_MUTATION_STARTED=true
apt-get update
apt-get install --yes ca-certificates git
VAPOR_RECOVERY_STAGING=1 VAPOR_RECOVERY_BUNDLE_SHA256="$BUNDLE_SHA256" \
  bash "$DEPLOY_ROOT/deploy/scripts/bootstrap-ubuntu.sh"
export CARGO_HOME="${TOOLCHAIN_ROOT}/cargo"
export RUSTUP_HOME="${TOOLCHAIN_ROOT}/rustup"
export PATH="${CARGO_HOME}/bin:${PATH}"
rustup toolchain install stable --profile minimal
rustup default stable
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-1}"
[[ "${CARGO_BUILD_JOBS}" =~ ^[1-9][0-9]*$ ]] || fail "CARGO_BUILD_JOBS must be a positive integer"
VAPOR_DEPLOY_BOOTSTRAP=1 VAPOR_RECOVERY_STAGING=1 \
  bash "$DEPLOY_ROOT/deploy/scripts/deploy.sh" --approved "$REVISION"
VAPOR_RECOVERY_STAGING=1 bash "$DEPLOY_ROOT/deploy/scripts/restore-state-only.sh" --bundle "$BUNDLE" --yes --fresh-host
# restore-state-only.sh held the deploy/state lock while importing data. Start
# scheduled exports only after that process exits and releases the lock, then
# run the complete health check with the production timer requirements.
systemctl start vapor-state-only-export.timer
"$DEPLOY_ROOT/deploy/scripts/health-check.sh"
python3 "$MARKER_TOOL" set-phase --root "$MARKER_ROOT" --bundle-sha256 "$BUNDLE_SHA256" \
  --root-commit "$REVISION" --phase awaiting_provider_checks
RECOVERY_AWAITING_CONFIRMATION=true

STATUS_URL="http://127.0.0.1:7113/v1/auth/status"
if [ "$SERVER_MODE" = monolith ]; then
  STATUS_URL="http://127.0.0.1:7110/api/identity/v1/auth/status"
fi
STATUS_FILE="$(mktemp /run/vapor-state-recovery-status.XXXXXX)"
curl --fail --silent --show-error --connect-timeout 3 --max-time 15 "$STATUS_URL" > "$STATUS_FILE" \
  || fail "Identity status endpoint did not respond after state restore"
python3 "$DEPLOY_ROOT/deploy/scripts/recovery_bundle_metadata.py" --check-provider-readiness "$STATUS_FILE"
echo "state-only-recovery: services restored; complete browser sign-in and owner-dashboard checks, then rerun with --resume --confirm-provider-checks"
exit 3
