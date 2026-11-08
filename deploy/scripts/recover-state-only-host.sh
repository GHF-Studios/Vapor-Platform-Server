#!/usr/bin/env bash
set -Eeuo pipefail
umask 077

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPOSITORY_URL="https://github.com/GHF-Studios/Vapor-Platform-Server.git"
DEPLOY_ROOT="/opt/vapor-platform-server"
STATE_ROOT="/var/lib/vapor-server"
CONFIG_ROOT="/etc/vapor-server"
TOOLCHAIN_ROOT="/opt/vapor/tools"

BUNDLE=""
CONFIRMED=false

usage() {
  cat <<'USAGE'
usage: recover-state-only-host.sh --bundle /path/to/state-snapshot.tar.gz --yes

Rebuilds a blank Ubuntu x86_64 Vapor host from source, re-enters external
provider credentials without echoing them, then imports the state-only archive.
The archive contains no host configuration or credentials. This is a one-shot
fresh-host operation; if interrupted after setup begins, inspect the host and
resume only after confirming the exact bundle and staged source checkout.
USAGE
}

fail() { echo "state-only-recovery: $*" >&2; exit 1; }

while [ "$#" -gt 0 ]; do
  case "$1" in
    --bundle) [ "$#" -ge 2 ] || { usage >&2; exit 2; }; BUNDLE="$2"; shift 2 ;;
    --yes) CONFIRMED=true; shift ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
  esac
done

[ "$(id -u)" -eq 0 ] || fail "run as root"
[ -n "$BUNDLE" ] && [ "$CONFIRMED" = true ] || { usage >&2; exit 2; }
[ -f "$BUNDLE" ] && [ ! -L "$BUNDLE" ] || fail "state snapshot must be a regular file"
[ -f "${BUNDLE}.sha256" ] && [ ! -L "${BUNDLE}.sha256" ] || fail "state snapshot checksum sidecar is missing"
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

require_empty_or_absent() {
  local path="$1"
  [ ! -L "$path" ] || fail "refusing symlink at ${path}"
  if [ -e "$path" ] && { [ ! -d "$path" ] || [ -n "$(find "$path" -mindepth 1 -maxdepth 1 -print -quit)" ]; }; then
    fail "${path} is not empty; reinstall or clean the host before fresh recovery"
  fi
}
require_empty_or_absent "$DEPLOY_ROOT"
require_empty_or_absent "$STATE_ROOT"
require_empty_or_absent "$CONFIG_ROOT"
require_empty_or_absent "$TOOLCHAIN_ROOT"
if [ -e /etc/caddy/Caddyfile ] || [ -L /etc/caddy/Caddyfile ]; then
  fail "existing Caddy configuration found; refusing to replace it"
fi

read -r -p 'Public domain [vapor.ghf-studios.site]: ' DOMAIN
DOMAIN="${DOMAIN:-vapor.ghf-studios.site}"
read -r -p "Admin domain [${DOMAIN}]: " ADMIN_DOMAIN
ADMIN_DOMAIN="${ADMIN_DOMAIN:-$DOMAIN}"
read -r -p 'Runtime mode [services]: ' SERVER_MODE
SERVER_MODE="${SERVER_MODE:-services}"
[ "$SERVER_MODE" = services ] || [ "$SERVER_MODE" = monolith ] || fail "runtime mode must be services or monolith"
[[ "$DOMAIN" =~ ^[A-Za-z0-9.-]+$ && "$ADMIN_DOMAIN" =~ ^[A-Za-z0-9.-]+$ ]] \
  || fail "domains contain unsupported characters"

read -r -s -p 'Steam Web API key (hidden): ' STEAM_KEY
printf '\n'
[[ "$STEAM_KEY" =~ ^[A-Fa-f0-9]{32}$ ]] || fail "Steam Web API key must be 32 hexadecimal characters"
read -r -p 'GitHub OAuth client ID (blank if not configured): ' GITHUB_CLIENT_ID
GITHUB_CLIENT_SECRET=""
if [ -n "$GITHUB_CLIENT_ID" ]; then
  [[ "$GITHUB_CLIENT_ID" =~ ^[A-Za-z0-9._-]{1,128}$ ]] || fail "GitHub client ID contains unsupported characters"
  read -r -s -p 'GitHub OAuth client secret (hidden): ' GITHUB_CLIENT_SECRET
  printf '\n'
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
export VAPOR_IDENTITY_STEAM_WEB_API_KEY="$STEAM_KEY"
export VAPOR_IDENTITY_GITHUB_CLIENT_ID="$GITHUB_CLIENT_ID"
export VAPOR_IDENTITY_GITHUB_CLIENT_SECRET="$GITHUB_CLIENT_SECRET"
unset STEAM_KEY GITHUB_CLIENT_ID GITHUB_CLIENT_SECRET

export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install --yes ca-certificates git
install -d -o root -g root -m 0755 /opt
TEMP_SOURCE="/opt/.vapor-state-recovery-${BUNDLE_SHA256:0:16}"
[ ! -e "$TEMP_SOURCE" ] && [ ! -L "$TEMP_SOURCE" ] || fail "recovery checkout path already exists; inspect before continuing"
git clone --no-checkout "$REPOSITORY_URL" "$TEMP_SOURCE"
git -C "$TEMP_SOURCE" fetch --no-tags origin "$REVISION"
git -C "$TEMP_SOURCE" checkout --detach "$REVISION"
git -C "$TEMP_SOURCE" submodule sync --recursive
git -C "$TEMP_SOURCE" submodule update --init --recursive
python3 "$TEMP_SOURCE/deploy/scripts/verify-deploy-source.py" --root "$TEMP_SOURCE" --commit "$REVISION"
mv -- "$TEMP_SOURCE" "$DEPLOY_ROOT"

VAPOR_RECOVERY_STAGING=1 VAPOR_RECOVERY_BUNDLE_SHA256="$BUNDLE_SHA256" \
  bash "$DEPLOY_ROOT/deploy/scripts/bootstrap-ubuntu.sh"
export CARGO_HOME="${TOOLCHAIN_ROOT}/cargo"
export RUSTUP_HOME="${TOOLCHAIN_ROOT}/rustup"
export PATH="${CARGO_HOME}/bin:${PATH}"
rustup toolchain install stable --profile minimal
rustup default stable
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
VAPOR_DEPLOY_BOOTSTRAP=1 VAPOR_RECOVERY_STAGING=1 \
  bash "$DEPLOY_ROOT/deploy/scripts/deploy.sh" --approved "$REVISION"

bash "$DEPLOY_ROOT/deploy/scripts/restore-state-only.sh" --bundle "$BUNDLE" --yes --fresh-host

STATUS_URL="http://127.0.0.1:7113/v1/auth/status"
if [ "$SERVER_MODE" = monolith ]; then
  STATUS_URL="http://127.0.0.1:7110/api/identity/v1/auth/status"
fi
STATUS_FILE="$(mktemp /run/vapor-state-recovery-status.XXXXXX)"
trap 'rm -f -- "$STATUS_FILE"' EXIT
curl --fail --silent --show-error --connect-timeout 3 --max-time 15 "$STATUS_URL" > "$STATUS_FILE" \
  || fail "Identity status endpoint did not respond after state restore"
python3 "$DEPLOY_ROOT/deploy/scripts/recovery_bundle_metadata.py" --check-provider-readiness "$STATUS_FILE"
echo "state-only-recovery: fresh host rebuilt, state restored, and configured provider readiness passed"
