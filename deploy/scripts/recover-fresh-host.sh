#!/usr/bin/env bash
set -Eeuo pipefail
umask 077

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly REPOSITORY_URL="https://github.com/GHF-Studios/Vapor-Platform-Server.git"
readonly DEPLOY_ROOT="/opt/vapor-platform-server"
readonly STATE_ROOT="/var/lib/vapor-server"
readonly CONFIG_ROOT="/etc/vapor-server"
readonly TOOLCHAIN_ROOT="/opt/vapor/tools"
readonly CARGO_HOME_PATH="${TOOLCHAIN_ROOT}/cargo"
readonly RUSTUP_HOME_PATH="${TOOLCHAIN_ROOT}/rustup"
readonly RECOVERY_MARKER_ROOT="/var/lib/vapor-recovery"
readonly RECOVERY_MARKER="${RECOVERY_MARKER_ROOT}/fresh-host.pin"

BUNDLE=""
CONFIRMED=false
RESUME=false
CONFIRM_PROVIDER_CHECKS=false
BOOTSTRAP_DIR=""
RECOVERY_COMPLETE=false
RECOVERY_STARTED=false
HOST_MUTATION_STARTED=false
RECOVERY_AWAITING_CONFIRMATION=false
STATUS_FILE=""
MARKER_TMP=""

usage() {
  cat <<'USAGE'
usage: recover-fresh-host.sh --bundle /path/to/recovery.tar.gz --yes [--resume]
       recover-fresh-host.sh --bundle /path/to/recovery.tar.gz --yes --resume --confirm-provider-checks

Rebuilds a fresh supported Ubuntu x86_64 host from a verified Vapor recovery
bundle. The operation installs packages, checks out the exact recorded source,
builds release binaries, restores the included state/configuration, and checks
configured provider readiness. Existing Vapor data/configuration is never
overwritten on a new run. If interrupted, rerun with --resume and the exact
same bundle to continue the marked recovery. After restoration, complete the
owner sign-in checks in a browser and rerun with --resume --confirm-provider-checks.
USAGE
}

fail() {
  echo "fresh-recovery: $*" >&2
  exit 1
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --bundle)
      [ "$#" -ge 2 ] || { usage >&2; exit 2; }
      BUNDLE="$2"
      shift 2
      ;;
    --yes)
      CONFIRMED=true
      shift
      ;;
    --resume)
      RESUME=true
      shift
      ;;
    --confirm-provider-checks)
      CONFIRM_PROVIDER_CHECKS=true
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *) usage >&2; exit 2 ;;
  esac
done

[ "$(id -u)" -eq 0 ] || fail "run as root"
[ -n "${BUNDLE}" ] && [ "${CONFIRMED}" = true ] || { usage >&2; exit 2; }
[ "${CONFIRM_PROVIDER_CHECKS}" = false ] || [ "${RESUME}" = true ] \
  || fail "--confirm-provider-checks requires --resume"
[ -f "${BUNDLE}" ] && [ ! -L "${BUNDLE}" ] || fail "bundle must be a regular file"
[ -f "${BUNDLE}.sha256" ] && [ ! -L "${BUNDLE}.sha256" ] || fail "bundle checksum sidecar is missing"
BUNDLE_SHA256="$(sha256sum "${BUNDLE}" | awk '{print $1}')"

# Validate the supplied archive before apt or any application-state changes.
# sha256sum is part of Ubuntu's base coreutils package.
sidecar_line="$(cat -- "${BUNDLE}.sha256")" || fail "bundle checksum sidecar cannot be read"
sidecar_digest="${sidecar_line%%  *}"
sidecar_name="${sidecar_line#*  }"
[[ "${sidecar_digest}" =~ ^[0-9a-f]{64}$ ]] || fail "bundle checksum sidecar digest is invalid"
[ "${sidecar_name}" = "$(basename -- "${BUNDLE}")" ] \
  || fail "bundle checksum sidecar names a different archive"
(cd -- "$(dirname -- "${BUNDLE}")" && sha256sum --check --status "$(basename -- "${BUNDLE}").sha256") \
  || fail "bundle checksum does not match"

if [ "${RESUME}" = true ]; then
  [ -d "${RECOVERY_MARKER_ROOT}" ] && [ ! -L "${RECOVERY_MARKER_ROOT}" ] \
    || fail "recovery marker directory is not a regular directory"
  [ "$(stat -c '%U:%a' "${RECOVERY_MARKER_ROOT}")" = root:700 ] \
    || fail "recovery marker directory ownership or permissions are invalid"
  [ -f "${RECOVERY_MARKER}" ] && [ ! -L "${RECOVERY_MARKER}" ] \
    || fail "--resume requires an unfinished recovery marker"
  [ "$(stat -c '%U:%a' "${RECOVERY_MARKER}")" = root:600 ] \
    || fail "recovery marker ownership or permissions are invalid"
  [ "$(wc -l < "${RECOVERY_MARKER}")" -eq 3 ] \
    || fail "recovery marker structure is invalid"
  grep -Fxq "bundle_sha256=${BUNDLE_SHA256}" "${RECOVERY_MARKER}" \
    || fail "resume bundle differs from the bundle used by the interrupted recovery"
  marker_phase="$(sed -n 's/^phase=//p' "${RECOVERY_MARKER}")"
  [ "$(grep -c '^phase=' "${RECOVERY_MARKER}")" -eq 1 ] \
    || fail "recovery marker phase is missing or ambiguous"
  case "${marker_phase}" in
    building|awaiting_provider_checks) ;;
    *) fail "recovery marker phase is unsupported" ;;
  esac
  RECOVERY_STARTED=true
else
  marker_phase=""
  [ ! -e "${RECOVERY_MARKER}" ] && [ ! -L "${RECOVERY_MARKER}" ] \
    || fail "an unfinished recovery marker exists; use --resume with the same bundle"
fi

. /etc/os-release
[ "${ID:-}" = ubuntu ] || fail "only Ubuntu is supported by this rebuild path"
[ "$(dpkg --print-architecture)" = amd64 ] || fail "only Ubuntu x86_64/amd64 is supported"

require_empty_or_absent() {
  local path="$1"
  if [ -L "${path}" ]; then fail "refusing symlink at ${path}"; fi
  if [ -e "${path}" ] && [ ! -d "${path}" ]; then fail "${path} exists and is not a directory"; fi
  if [ -e "${path}" ] && [ -n "$(find "${path}" -mindepth 1 -maxdepth 1 -print -quit 2>/dev/null)" ]; then
    fail "${path} is not empty; this command only runs on a fresh host"
  fi
}

if [ "${RESUME}" = false ]; then
  require_empty_or_absent "${DEPLOY_ROOT}"
  require_empty_or_absent "${STATE_ROOT}"
  require_empty_or_absent "${CONFIG_ROOT}"
  require_empty_or_absent "${RECOVERY_MARKER_ROOT}"
  if [ -e /etc/caddy/Caddyfile ] || [ -L /etc/caddy/Caddyfile ]; then
    fail "an existing Caddyfile was found; refusing to replace host routing"
  fi
fi

if [ "${RESUME}" = false ] || [ "${marker_phase}" != awaiting_provider_checks ]; then
  python3 -c 'import tarfile, tomllib' >/dev/null 2>&1 \
    || fail "preflight requires the Ubuntu-provided Python 3.11+ runtime; no packages were changed"
  (cd -- "${SCRIPT_DIR}" && sha256sum --check --status SHA256SUMS) \
    || fail "recovery-kit files do not match their pinned checksums"
  REVISION="$(python3 "${SCRIPT_DIR}/recovery_bundle_metadata.py" --source-commit "${BUNDLE}")" \
    || fail "bundle source commit could not be read"
  python3 "${SCRIPT_DIR}/state-bundle-validate.py" "${BUNDLE}" \
    || fail "bundle validation failed before package changes"
  python3 "${SCRIPT_DIR}/recovery_bundle_metadata.py" \
    --check-host "${BUNDLE}" "${VERSION_ID:-}" x86_64 x86_64-unknown-linux-gnu \
    || fail "bundle host/toolchain is incompatible; no packages were changed"

  export DEBIAN_FRONTEND=noninteractive
  apt-get update
  apt-get install --yes ca-certificates curl git python3
fi

install -d -o root -g root -m 0755 /opt
BOOTSTRAP_DIR="/opt/.vapor-recovery-bootstrap-${BUNDLE_SHA256:0:16}"
if [ -L "${BOOTSTRAP_DIR}" ] || { [ -e "${BOOTSTRAP_DIR}" ] && [ ! -d "${BOOTSTRAP_DIR}" ]; }; then
  fail "recovery bootstrap path is not a regular directory"
fi
if [ "${RESUME}" = false ] && [ -e "${BOOTSTRAP_DIR}" ]; then
  fail "recovery bootstrap path already exists; inspect it before proceeding"
fi
install -d -o root -g root -m 0700 "${BOOTSTRAP_DIR}"
cleanup() {
  local status="${1:-$?}"
  trap - EXIT
  if [ -n "${STATUS_FILE}" ]; then rm -f -- "${STATUS_FILE}"; fi
  if [ -n "${MARKER_TMP}" ]; then rm -f -- "${MARKER_TMP}"; fi
  if [ "${status}" -ne 0 ] && [ "${HOST_MUTATION_STARTED}" = true ] \
    && [ "${RECOVERY_AWAITING_CONFIRMATION}" = false ]; then
    for unit in \
      vapor-homepage.service vapor-docs.service vapor-identity.service \
      vapor-diagnostics.service vapor-registry.service vapor-artifact.service \
      vapor-build-cache.service vapor-platform-server.service \
      vapor-state-export.timer vapor-state-only-export.timer caddy.service; do
      systemctl disable --now "${unit}" >/dev/null 2>&1 || true
    done
    echo "fresh-recovery: failed recovery units were disabled and stopped; rerun --resume after review" >&2
  fi
  if [ "${RECOVERY_COMPLETE}" = true ] || [ "${RECOVERY_STARTED}" = false ]; then
    rm -rf -- "${BOOTSTRAP_DIR}"
    if [ "${RECOVERY_STARTED}" = false ] && [ -d "${RECOVERY_MARKER_ROOT}" ] \
      && [ ! -e "${RECOVERY_MARKER}" ]; then
      rmdir -- "${RECOVERY_MARKER_ROOT}" 2>/dev/null || true
    fi
  elif [ "${RECOVERY_AWAITING_CONFIRMATION}" = true ]; then
    echo "fresh-recovery: waiting for owner provider checks; recovery marker retained" >&2
  else
    echo "fresh-recovery: stopped with status ${status}; bootstrap checkout retained at ${BOOTSTRAP_DIR}" >&2
  fi
  exit "${status}"
}
trap cleanup EXIT

if [ "${RESUME}" = true ] && [ "${marker_phase}" = awaiting_provider_checks ]; then
  [ -d "${DEPLOY_ROOT}/.git" ] && [ ! -L "${DEPLOY_ROOT}" ] \
    || fail "provider-check resume requires the completed deployment checkout"
  [ -f "${DEPLOY_ROOT}/deploy/scripts/recovery_bundle_metadata.py" ] \
    || fail "provider readiness checker is missing from the deployed source"
  confirmed_revision="$(python3 "${DEPLOY_ROOT}/deploy/scripts/recovery_bundle_metadata.py" --source-commit "${BUNDLE}")" \
    || fail "resume bundle source pin could not be read"
  grep -Fxq "root_commit=${confirmed_revision}" "${RECOVERY_MARKER}" \
    || fail "resume bundle source pin differs from the completed recovery"
  [ -f "${CONFIG_ROOT}/platform.env" ] && [ ! -L "${CONFIG_ROOT}/platform.env" ] \
    || fail "restored Platform configuration is missing or unsafe"
  if grep -q '^VAPOR_SERVER_MODE=services$' "${CONFIG_ROOT}/platform.env"; then
    IDENTITY_STATUS_URL=http://127.0.0.1:7113/v1/auth/status
  else
    IDENTITY_STATUS_URL=http://127.0.0.1:7110/api/identity/v1/auth/status
  fi
  STATUS_FILE="$(mktemp /run/vapor-recovery-identity-status.XXXXXX)"
  curl --fail --silent --show-error --connect-timeout 3 --max-time 15 \
    "${IDENTITY_STATUS_URL}" > "${STATUS_FILE}" \
    || fail "Identity status endpoint did not respond after restore"
  python3 "${DEPLOY_ROOT}/deploy/scripts/recovery_bundle_metadata.py" \
    --check-provider-readiness "${STATUS_FILE}"
  if [ "${CONFIRM_PROVIDER_CHECKS}" != true ]; then
    RECOVERY_AWAITING_CONFIRMATION=true
    echo "fresh-recovery: restore is complete, but owner provider checks remain pending"
    echo "fresh-recovery: complete configured provider checks in a browser, verify the owner dashboard, and rerun with --resume --confirm-provider-checks"
    exit 3
  fi
  unexpected_marker_entry="$(find "${RECOVERY_MARKER_ROOT}" -mindepth 1 -maxdepth 1 ! -name fresh-host.pin -print -quit)"
  [ -z "${unexpected_marker_entry}" ] || fail "unexpected files exist in the recovery marker directory"
  rm -f -- "${RECOVERY_MARKER}"
  rmdir -- "${RECOVERY_MARKER_ROOT}"
  RECOVERY_COMPLETE=true
  echo "fresh-recovery: owner-confirmed provider checks and configured readiness are complete"
  exit 0
fi

# The bounded source pin was read from the checksummed kit and the full bundle
# was prevalidated before package changes. The exact pinned checkout validates
# the same archive again before any application state is changed.
if [ -e "${DEPLOY_ROOT}" ]; then
  [ "${RESUME}" = true ] && [ -d "${DEPLOY_ROOT}/.git" ] && [ ! -L "${DEPLOY_ROOT}" ] \
    || fail "deployment path exists outside a resumable recovery checkout"
  [ "$(git -C "${DEPLOY_ROOT}" remote get-url origin)" = "${REPOSITORY_URL}" ] \
    || fail "resume checkout repository identity changed"
  [ "$(git -C "${DEPLOY_ROOT}" rev-parse HEAD)" = "${REVISION}" ] \
    || fail "resume checkout revision differs from the recovery bundle"
  [ -z "$(git -C "${DEPLOY_ROOT}" status --porcelain --untracked-files=all)" ] \
    || fail "resume checkout contains local changes"
  VALIDATOR_ROOT="${DEPLOY_ROOT}"
else
  TEMP_SOURCE="${BOOTSTRAP_DIR}/platform"
  if [ -e "${TEMP_SOURCE}" ]; then
    [ -d "${TEMP_SOURCE}/.git" ] && [ ! -L "${TEMP_SOURCE}" ] \
      || fail "interrupted temporary checkout is not a regular Git checkout"
    [ "$(git -C "${TEMP_SOURCE}" remote get-url origin)" = "${REPOSITORY_URL}" ] \
      || fail "temporary checkout repository identity changed"
  else
    git clone --no-checkout "${REPOSITORY_URL}" "${TEMP_SOURCE}"
  fi
  git -C "${TEMP_SOURCE}" fetch --no-tags origin "${REVISION}"
  git -C "${TEMP_SOURCE}" checkout --detach "${REVISION}"
  VALIDATOR_ROOT="${TEMP_SOURCE}"
fi
python3 "${VALIDATOR_ROOT}/deploy/scripts/state-bundle-validate.py" "${BUNDLE}"
python3 "${VALIDATOR_ROOT}/deploy/scripts/recovery_bundle_metadata.py" --describe "${BUNDLE}" \
  > "${BOOTSTRAP_DIR}/metadata.json"

read_metadata() {
  python3 - "${BOOTSTRAP_DIR}/metadata.json" "$1" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as source:
    metadata = json.load(source)
value = metadata.get(sys.argv[2])
if not isinstance(value, str) or not value:
    raise SystemExit(1)
print(value)
PY
}

MODE="$(read_metadata runtime_mode)" || fail "bundle runtime mode is unavailable"
METADATA_REVISION="$(read_metadata root_commit)" || fail "bundle source revision is unavailable"
[ "${REVISION}" = "${METADATA_REVISION}" ] || fail "bootstrap source pin disagrees with the pinned metadata"
DOMAIN="$(read_metadata domain)" || fail "bundle domain is unavailable"
RUSTC_VERSION="$(read_metadata rustc_version)" || fail "bundle has no exact Rust compiler observation; export a current recovery bundle"
RECORDED_CARGO_VERSION="$(read_metadata cargo_version)" || fail "bundle has no exact Cargo observation; export a current recovery bundle"
RUST_HOST="$(read_metadata rust_host_triple)" || fail "bundle has no Rust host observation; export a current recovery bundle"
RECORDED_ARCH="$(read_metadata architecture)" || fail "bundle has no host architecture observation"
RECORDED_OS="$(read_metadata operating_system)" || fail "bundle has no operating-system observation"
[ "${RUST_HOST}" = x86_64-unknown-linux-gnu ] || fail "bundle compiler host does not match this Ubuntu x86_64 recovery target"
[ "${RECORDED_ARCH}" = x86_64 ] || fail "bundle host architecture does not match this Ubuntu x86_64 recovery target"
recorded_ubuntu_release="$(printf '%s\n' "${RECORDED_OS}" | sed -E 's/^Ubuntu ([0-9]+\.[0-9]+).*/\1/')"
current_ubuntu_release="$(printf '%s' "${VERSION_ID:-}" | cut -d. -f1-2)"
[ "${recorded_ubuntu_release}" = "${current_ubuntu_release}" ] \
  || fail "bundle was exported from Ubuntu ${recorded_ubuntu_release}; this host is Ubuntu ${current_ubuntu_release}"

if [ "${RESUME}" = true ]; then
  grep -Fxq "root_commit=${REVISION}" "${RECOVERY_MARKER}" \
    || fail "resume source revision differs from the interrupted recovery"
else
  install -d -o root -g root -m 0700 "${RECOVERY_MARKER_ROOT}"
  MARKER_TMP="$(mktemp "${RECOVERY_MARKER_ROOT}/fresh-host.pin.XXXXXX")"
  printf 'bundle_sha256=%s\nroot_commit=%s\nphase=building\n' "${BUNDLE_SHA256}" "${REVISION}" > "${MARKER_TMP}"
  chown root:root "${MARKER_TMP}"
  chmod 0600 "${MARKER_TMP}"
  mv -f -- "${MARKER_TMP}" "${RECOVERY_MARKER}"
  MARKER_TMP=""
  RECOVERY_STARTED=true
fi

if [ "${VALIDATOR_ROOT}" != "${DEPLOY_ROOT}" ]; then
  [ ! -e "${DEPLOY_ROOT}" ] && [ ! -L "${DEPLOY_ROOT}" ] \
    || fail "deployment path appeared during recovery; refusing to replace it"
  mv -- "${VALIDATOR_ROOT}" "${DEPLOY_ROOT}"
  VALIDATOR_ROOT="${DEPLOY_ROOT}"
fi
git -C "${DEPLOY_ROOT}" submodule sync --recursive
git -C "${DEPLOY_ROOT}" submodule update --init --recursive

python3 - "${BUNDLE}" "${DEPLOY_ROOT}" <<'PY'
import subprocess
import sys
import tarfile

bundle, root = sys.argv[1:]
try:
    with tarfile.open(bundle, "r:gz") as archive:
        expected_revision = archive.extractfile(
            "vapor-platform-recovery/source/root-commit.txt"
        ).read(256).decode("ascii").strip()
        expected_submodules = archive.extractfile(
            "vapor-platform-recovery/source/submodules.txt"
        ).read(4 * 1024 * 1024).decode("utf-8")
    revision = subprocess.check_output(["git", "-C", root, "rev-parse", "HEAD"], text=True).strip()
    submodules = subprocess.check_output(
        ["git", "-C", root, "submodule", "status", "--recursive"], text=True
    )
    dirty = subprocess.check_output(
        ["git", "-C", root, "status", "--porcelain", "--untracked-files=all"], text=True
    )
    nested_dirty = subprocess.run(
        ["git", "-C", root, "submodule", "foreach", "--quiet", "--recursive",
         'test -z "$(git status --porcelain --untracked-files=all)"'],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False,
    ).returncode
except (OSError, subprocess.CalledProcessError, tarfile.TarError, AttributeError) as error:
    raise SystemExit("fresh-recovery: source/submodule pin verification failed") from error
if revision != expected_revision or submodules != expected_submodules or dirty or nested_dirty:
    raise SystemExit("fresh-recovery: checkout differs from the clean recorded source pins")
PY

export VAPOR_SERVER_MODE="${MODE}"
export VAPOR_DOMAIN="${DOMAIN}"
export VAPOR_BRANCH=main
export VAPOR_REPO_URL="${REPOSITORY_URL}"
export VAPOR_DEPLOY_ROOT="${DEPLOY_ROOT}"
export VAPOR_STATE_ROOT="${STATE_ROOT}"
export VAPOR_CONFIG_DIR="${CONFIG_ROOT}"
export VAPOR_TOOLCHAIN_ROOT="${TOOLCHAIN_ROOT}"
export VAPOR_CARGO_HOME="${CARGO_HOME_PATH}"
export VAPOR_RUSTUP_HOME="${RUSTUP_HOME_PATH}"

VAPOR_RECOVERY_STAGING=1 VAPOR_RECOVERY_BUNDLE_SHA256="${BUNDLE_SHA256}" \
  bash "${DEPLOY_ROOT}/deploy/scripts/bootstrap-ubuntu.sh"
export CARGO_HOME="${CARGO_HOME_PATH}"
export RUSTUP_HOME="${RUSTUP_HOME_PATH}"
export PATH="${CARGO_HOME_PATH}/bin:${PATH}"
rustup toolchain install "${RUSTC_VERSION}" --profile minimal
rustup default "${RUSTC_VERSION}"
actual_rustc="$(rustc --version | awk '{print $2}')"
actual_cargo="$(cargo --version | awk '{print $2}')"
[ "${actual_rustc}" = "${RUSTC_VERSION}" ] || fail "installed rustc version does not match the recorded version"
[ "${actual_cargo}" = "${RECORDED_CARGO_VERSION}" ] || fail "installed Cargo version does not match the recorded version"

export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-1}"
[[ "${CARGO_BUILD_JOBS}" =~ ^[1-9][0-9]*$ ]] || fail "CARGO_BUILD_JOBS must be a positive integer"
export VAPOR_PLATFORM_REVISION="${REVISION}"
HOST_MUTATION_STARTED=true
VAPOR_DEPLOY_BOOTSTRAP=1 VAPOR_RECOVERY_STAGING=1 \
  bash "${DEPLOY_ROOT}/deploy/scripts/deploy.sh" --approved "${REVISION}"
bash "${DEPLOY_ROOT}/deploy/scripts/restore-state.sh" --bundle "${BUNDLE}" --yes --fresh-host

MARKER_TMP="$(mktemp "${RECOVERY_MARKER_ROOT}/fresh-host.pin.XXXXXX")"
printf 'bundle_sha256=%s\nroot_commit=%s\nphase=awaiting_provider_checks\n' "${BUNDLE_SHA256}" "${REVISION}" > "${MARKER_TMP}"
chown root:root "${MARKER_TMP}"
chmod 0600 "${MARKER_TMP}"
mv -f -- "${MARKER_TMP}" "${RECOVERY_MARKER}"
MARKER_TMP=""
RECOVERY_AWAITING_CONFIRMATION=true

if [ "${MODE}" = services ]; then
  IDENTITY_STATUS_URL=http://127.0.0.1:7113/v1/auth/status
else
  IDENTITY_STATUS_URL=http://127.0.0.1:7110/api/identity/v1/auth/status
fi
STATUS_FILE="$(mktemp /run/vapor-recovery-identity-status.XXXXXX)"
trap 'status=$?; cleanup "${status}"' EXIT
curl --fail --silent --show-error --connect-timeout 3 --max-time 15 \
  "${IDENTITY_STATUS_URL}" > "${STATUS_FILE}" \
  || fail "Identity status endpoint did not respond after restore"
python3 "${VALIDATOR_ROOT}/deploy/scripts/recovery_bundle_metadata.py" \
  --check-provider-readiness "${STATUS_FILE}"

echo "fresh-recovery: services restored and all configured provider/dashboard readiness checks passed"
echo "fresh-recovery: complete configured provider checks in a browser, verify the owner dashboard, and rerun with --resume --confirm-provider-checks"
exit 3
