#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "${SCRIPT_DIR}/lib.sh"

require_root

BACKUP_ROOT="${VAPOR_BACKUP_ROOT:-/var/backups/vapor-server}"
BACKUP_RETENTION_COUNT="${VAPOR_BACKUP_RETENTION_COUNT:-168}"
if [[ ! "${BACKUP_RETENTION_COUNT}" =~ ^[0-9]+$ ]] || [ "${BACKUP_RETENTION_COUNT}" -lt 2 ]; then
  echo "error: VAPOR_BACKUP_RETENTION_COUNT must be an integer of at least 2" >&2
  exit 1
fi
TIMESTAMP="$(date -u +%Y%m%dT%H%M%SZ)"
DEPLOY_COMMIT="$(git -C "${VAPOR_DEPLOY_ROOT}" rev-parse HEAD 2>/dev/null || printf 'unknown')"
MODE="${VAPOR_SERVER_MODE:-monolith}"
BUNDLE_NAME="vapor-platform-recovery-${TIMESTAMP}-${DEPLOY_COMMIT:0:12}"
BUNDLE_PATH="${1:-${BACKUP_ROOT}/${BUNDLE_NAME}.tar.gz}"
BUNDLE_DIR="$(dirname -- "${BUNDLE_PATH}")"
ACTIVE_SERVICES=()

if [ "${BUNDLE_DIR}" != "${BACKUP_ROOT}" ]; then
  echo "error: recovery archives must be published directly under VAPOR_BACKUP_ROOT" >&2
  exit 1
fi
if [[ ! "${DEPLOY_COMMIT}" =~ ^[0-9a-f]{40}$ ]]; then
  echo "error: cannot export recovery without an exact 40-character Git commit" >&2
  exit 1
fi
if ! command -v rustc >/dev/null 2>&1 || ! command -v cargo >/dev/null 2>&1; then
  echo "error: cannot record the Rust/Cargo versions present on the export host" >&2
  exit 1
fi
RUSTC_VERSION="$(rustc --version | awk '{print $2}')"
CARGO_VERSION="$(cargo --version | awk '{print $2}')"
RUST_HOST_TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"
if [[ ! "${RUSTC_VERSION}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || \
  [[ ! "${CARGO_VERSION}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || \
  [[ ! "${RUST_HOST_TRIPLE}" =~ ^[A-Za-z0-9_]+(-[A-Za-z0-9_]+){2,}$ ]]; then
  echo "error: compiler toolchain version or host triple is not a supported stable release" >&2
  exit 1
fi
if [ -n "$(git -C "${VAPOR_DEPLOY_ROOT}" status --porcelain --untracked-files=all)" ]; then
  echo "error: cannot export recovery with a dirty Platform source checkout" >&2
  exit 1
fi
if ! git -C "${VAPOR_DEPLOY_ROOT}" submodule foreach --quiet --recursive \
  'test -z "$(git status --porcelain --untracked-files=all)"' >/dev/null; then
  echo "error: cannot export recovery with a dirty service submodule checkout" >&2
  exit 1
fi
if [ -e "${BUNDLE_PATH}" ] || [ -e "${BUNDLE_PATH}.sha256" ]; then
  echo "error: export target already exists" >&2
  exit 1
fi
case "${BACKUP_RETENTION_COUNT}" in
  ""|*[!0-9]*) echo "error: VAPOR_BACKUP_RETENTION_COUNT must be a positive integer" >&2; exit 1 ;;
esac
if [ "${BACKUP_RETENTION_COUNT}" -lt 1 ]; then
  echo "error: VAPOR_BACKUP_RETENTION_COUNT must be at least 1" >&2
  exit 1
fi
case "${MODE}" in
  monolith|services)
    STATEFUL_SERVICES=(
      vapor-homepage.service vapor-docs.service vapor-identity.service
      vapor-diagnostics.service vapor-registry.service vapor-artifact.service
      vapor-build-cache.service vapor-platform-server.service
    )
    ;;
  *) echo "error: unsupported VAPOR_SERVER_MODE" >&2; exit 1 ;;
esac

if ! command -v flock >/dev/null 2>&1; then
  echo "error: flock is required for state export locking" >&2
  exit 1
fi

install_dir root root 0700 "${BACKUP_ROOT}"

LOCK_FILE=/run/vapor-server-deploy.lock
exec 9>"${LOCK_FILE}"
if ! flock -n 9; then
  echo "error: Vapor deploy/state lock is already held; retry after the active operation finishes" >&2
  exit 1
fi
if [ "$(git -C "${VAPOR_DEPLOY_ROOT}" rev-parse HEAD)" != "${DEPLOY_COMMIT}" ] || \
  [ -n "$(git -C "${VAPOR_DEPLOY_ROOT}" status --porcelain --untracked-files=all)" ] || \
  ! git -C "${VAPOR_DEPLOY_ROOT}" submodule foreach --quiet --recursive \
    'test -z "$(git status --porcelain --untracked-files=all)"' >/dev/null; then
  echo "error: Platform source changed while acquiring the recovery lock; retry the export" >&2
  exit 1
fi

TMP_DIR="$(mktemp -d "${BACKUP_ROOT}/.export.XXXXXX")"
BUNDLE_ROOT="${TMP_DIR}/vapor-platform-recovery"
mkdir -m 0700 "${BUNDLE_ROOT}"

cleanup() {
  rm -rf -- "${TMP_DIR}"
}

restart_services() {
  local service
  local failed=0
  for service in "${ACTIVE_SERVICES[@]}"; do
    if ! systemctl start "${service}"; then
      echo "error: could not restart a service after recovery snapshot" >&2
      failed=1
    fi
  done
  return "${failed}"
}

finish() {
  local status=$?
  trap - EXIT
  set +e
  restart_services
  cleanup
  exit "${status}"
}
trap finish EXIT

mkdir -m 0700 "${BUNDLE_ROOT}/state" "${BUNDLE_ROOT}/config" \
  "${BUNDLE_ROOT}/host" "${BUNDLE_ROOT}/source"
mkdir -m 0700 "${BUNDLE_ROOT}/config/vapor-server" \
  "${BUNDLE_ROOT}/host/caddy" "${BUNDLE_ROOT}/host/systemd"
mkdir -m 0700 "${BUNDLE_ROOT}/source/recovery-kit"

for service in "${STATEFUL_SERVICES[@]}"; do
  if systemctl is-active --quiet "${service}"; then
    ACTIVE_SERVICES+=("${service}")
    systemctl stop "${service}"
  fi
done

if [ -d "${VAPOR_STATE_ROOT}" ]; then
  tar \
    --exclude='./build-cache/targets' \
    --exclude='./build-cache/staging' \
    -C "${VAPOR_STATE_ROOT}" -cf - . \
    | tar -C "${BUNDLE_ROOT}/state" -xf -
fi

if [ -d "${VAPOR_CONFIG_DIR}" ]; then
  cp -a "${VAPOR_CONFIG_DIR}/." "${BUNDLE_ROOT}/config/vapor-server/"
fi

copy_host_file() {
  local source_path="$1"
  local destination="$2"
  if [ -f "${source_path}" ] && [ ! -L "${source_path}" ]; then
    install -D -o root -g root -m 0644 "${source_path}" "${BUNDLE_ROOT}/${destination}"
  fi
}

copy_host_file /etc/caddy/Caddyfile host/caddy/Caddyfile
if [ ! -f "${BUNDLE_ROOT}/host/caddy/Caddyfile" ]; then
  echo "error: active Caddy configuration is missing; refusing incomplete recovery archive" >&2
  exit 1
fi
HOST_UNIT_COUNT=0
for unit in /etc/systemd/system/vapor-*.service /etc/systemd/system/vapor-*.timer; do
  [ -e "${unit}" ] || continue
  if [ ! -f "${unit}" ] || [ -L "${unit}" ]; then
    echo "error: a Vapor systemd unit is not a regular file; refusing incomplete recovery archive" >&2
    exit 1
  fi
  case "$(basename -- "${unit}")" in
    vapor-deploy.service|vapor-deploy.timer) continue ;;
  esac
  copy_host_file "${unit}" "host/systemd/$(basename -- "${unit}")"
  HOST_UNIT_COUNT=$((HOST_UNIT_COUNT + 1))
done
if [ "${HOST_UNIT_COUNT}" -lt 1 ]; then
  echo "error: no Vapor systemd unit files found; refusing incomplete recovery archive" >&2
  exit 1
fi

printf '%s\n' "${DEPLOY_COMMIT}" > "${BUNDLE_ROOT}/source/root-commit.txt"
printf 'rustc_version=%s\ncargo_version=%s\nhost_triple=%s\n' \
  "${RUSTC_VERSION}" "${CARGO_VERSION}" "${RUST_HOST_TRIPLE}" \
  > "${BUNDLE_ROOT}/source/toolchain.txt"
git -C "${VAPOR_DEPLOY_ROOT}" submodule status --recursive \
  > "${BUNDLE_ROOT}/source/submodules.txt"
if grep -Eq '^[-+U]' "${BUNDLE_ROOT}/source/submodules.txt"; then
  echo "error: recursive submodule checkout does not match the recorded source revision" >&2
  exit 1
fi

RECOVERY_KIT_DIR="${BUNDLE_ROOT}/source/recovery-kit"
for recovery_file in \
  recover-fresh-host.sh recovery_bundle_metadata.py \
  state-bundle-validate.py deployed_source_manifest.py README.txt; do
  source_path="${SCRIPT_DIR}/${recovery_file}"
  [ -f "${source_path}" ] && [ ! -L "${source_path}" ] \
    || { echo "error: required recovery kit source file is missing" >&2; exit 1; }
  install -o root -g root -m 0600 "${source_path}" "${RECOVERY_KIT_DIR}/${recovery_file}"
done
(cd "${RECOVERY_KIT_DIR}" && sha256sum \
  README.txt recover-fresh-host.sh recovery_bundle_metadata.py \
  state-bundle-validate.py deployed_source_manifest.py) \
  > "${RECOVERY_KIT_DIR}/SHA256SUMS"
chmod 0600 "${RECOVERY_KIT_DIR}/SHA256SUMS"

export VAPOR_RECOVERY_TIMESTAMP="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
export VAPOR_RECOVERY_DEPLOY_COMMIT="${DEPLOY_COMMIT}"
export VAPOR_RECOVERY_BRANCH="${VAPOR_BRANCH:-unknown}"
export VAPOR_RECOVERY_MODE="${MODE}"
export VAPOR_RECOVERY_STATE_ROOT="${VAPOR_STATE_ROOT}"
export VAPOR_RECOVERY_CONFIG_DIR="${VAPOR_CONFIG_DIR}"
export VAPOR_RECOVERY_OS="$(. /etc/os-release && printf '%s' "${PRETTY_NAME:-unknown}")"
export VAPOR_RECOVERY_ARCH="$(uname -m)"
export VAPOR_RECOVERY_RUSTC_VERSION="${RUSTC_VERSION}"
export VAPOR_RECOVERY_CARGO_VERSION="${CARGO_VERSION}"
export VAPOR_RECOVERY_RUST_HOST_TRIPLE="${RUST_HOST_TRIPLE}"
python3 - "${BUNDLE_ROOT}/manifest.toml" <<'PY'
import json
import os
import sys

def quote(value: str) -> str:
    return json.dumps(value, ensure_ascii=True)

manifest = {
    "schema_version": "2",
    "created_at_utc": quote(os.environ["VAPOR_RECOVERY_TIMESTAMP"]),
    "runtime_mode": quote(os.environ["VAPOR_RECOVERY_MODE"]),
    "deploy_root_commit": quote(os.environ["VAPOR_RECOVERY_DEPLOY_COMMIT"]),
    "branch": quote(os.environ["VAPOR_RECOVERY_BRANCH"]),
    "state_root": quote(os.environ["VAPOR_RECOVERY_STATE_ROOT"]),
    "config_dir": quote(os.environ["VAPOR_RECOVERY_CONFIG_DIR"]),
    "operating_system": quote(os.environ["VAPOR_RECOVERY_OS"]),
    "architecture": quote(os.environ["VAPOR_RECOVERY_ARCH"]),
    "rustc_version": quote(os.environ["VAPOR_RECOVERY_RUSTC_VERSION"]),
    "cargo_version": quote(os.environ["VAPOR_RECOVERY_CARGO_VERSION"]),
    "rust_host_triple": quote(os.environ["VAPOR_RECOVERY_RUST_HOST_TRIPLE"]),
    "secrets_included": "true",
    "archive_encryption": quote("none"),
    "build_outputs_excluded": "true",
    "source_pins_included": "true",
    "config_included": "true",
    "recovery_kit_included": "true",
    "host_routing_included": "true",
    "included_categories": '["state", "config", "host", "source"]',
}
with open(sys.argv[1], "w", encoding="utf-8", newline="\n") as output:
    for key, value in manifest.items():
        output.write(f"{key} = {value}\n")
PY

if ! restart_services; then
  echo "error: archive copied, but one or more services did not return after quiescing" >&2
  exit 1
fi
ACTIVE_SERVICES=()

ARCHIVE_TMP="${TMP_DIR}/bundle.tar.gz"
tar -C "${TMP_DIR}" -czf "${ARCHIVE_TMP}" vapor-platform-recovery
python3 "${SCRIPT_DIR}/state-bundle-validate.py" "${ARCHIVE_TMP}"
ARCHIVE_SHA256="$(sha256sum "${ARCHIVE_TMP}" | awk '{print $1}')"
ARCHIVE_SIZE="$(stat -c '%s' "${ARCHIVE_TMP}")"
FINAL_SHA="${BUNDLE_PATH}.sha256"

mv -- "${ARCHIVE_TMP}" "${BUNDLE_PATH}"
printf '%s  %s\n' "${ARCHIVE_SHA256}" "$(basename -- "${BUNDLE_PATH}")" > "${TMP_DIR}/bundle.sha256"
chmod 0600 "${BUNDLE_PATH}" "${TMP_DIR}/bundle.sha256"
mv -- "${TMP_DIR}/bundle.sha256" "${FINAL_SHA}"
PRUNE_PLAN="${TMP_DIR}/prune-list.txt"
python3 "${SCRIPT_DIR}/plan_recovery_retention.py" \
  --root "${BACKUP_ROOT}" \
  --new "$(basename -- "${BUNDLE_PATH}")" \
  --keep "${BACKUP_RETENTION_COUNT}" \
  --output "${PRUNE_PLAN}"

python3 - "${BACKUP_ROOT}" "${TMP_DIR}/index.json" "${PRUNE_PLAN}" <<'PY'
import json
import datetime
import hashlib
import pathlib
import re
import sys

root, index_path, prune_path = map(pathlib.Path, sys.argv[1:])
pruned = set(prune_path.read_text(encoding="ascii").splitlines())
records = []
for archive in sorted(root.glob("vapor-platform-recovery-*.tar.gz")):
    if archive.name in pruned or archive.is_symlink() or not archive.is_file():
        continue
    match = re.fullmatch(r"vapor-platform-recovery-(\d{8}T\d{6}Z)-[0-9a-f]{12}\.tar\.gz", archive.name)
    if match is None:
        continue
    sidecar = pathlib.Path(f"{archive}.sha256")
    if sidecar.is_symlink() or not sidecar.is_file():
        continue
    try:
        digest, recorded_name = sidecar.read_text(encoding="ascii").strip().split(maxsplit=1)
    except (OSError, ValueError):
        continue
    if recorded_name != archive.name or not re.fullmatch(r"[0-9a-f]{64}", digest):
        continue
    size_bytes = archive.stat().st_size
    if size_bytes <= 0:
        continue
    actual_digest = hashlib.sha256()
    with archive.open("rb") as contents:
        for chunk in iter(lambda: contents.read(1024 * 1024), b""):
            actual_digest.update(chunk)
    if actual_digest.hexdigest() != digest:
        continue
    records.append({
        "archive": archive.name,
        "sha256": digest,
        "size_bytes": size_bytes,
        "created_at_utc": datetime.datetime.strptime(
            match.group(1), "%Y%m%dT%H%M%SZ"
        ).replace(tzinfo=datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    })
if not records:
    raise SystemExit("recovery-export: no complete archive generations found")
with open(index_path, "w", encoding="utf-8", newline="\n") as output:
    json.dump({"schema_version": 1, "archives": records}, output, separators=(",", ":"))
    output.write("\n")
PY
chmod 0600 "${TMP_DIR}/index.json"
mv -- "${TMP_DIR}/index.json" "${BACKUP_ROOT}/index.json"

LATEST_TMP="${TMP_DIR}/latest.json"
python3 - "${LATEST_TMP}" "$(basename -- "${BUNDLE_PATH}")" \
  "${ARCHIVE_SHA256}" "${ARCHIVE_SIZE}" "${VAPOR_RECOVERY_TIMESTAMP}" <<'PY'
import json
import sys

path, archive, digest, size, created_at = sys.argv[1:]
with open(path, "w", encoding="utf-8", newline="\n") as output:
    json.dump({
        "schema_version": 1,
        "archive": archive,
        "sha256": digest,
        "size_bytes": int(size),
        "created_at_utc": created_at,
        "recovery_kit_included": True,
    }, output, separators=(",", ":"))
    output.write("\n")
PY
chmod 0600 "${LATEST_TMP}"
mv -- "${LATEST_TMP}" "${BACKUP_ROOT}/latest.json"

while IFS= read -r pruned_name; do
  [ -n "${pruned_name}" ] || continue
  [[ "${pruned_name}" =~ ^vapor-platform-recovery-[0-9]{8}T[0-9]{6}Z-[0-9a-f]{12}\.tar\.gz$ ]] \
    || { echo "error: retention planner returned an invalid archive name" >&2; exit 1; }
  rm -f -- "${BACKUP_ROOT}/${pruned_name}" "${BACKUP_ROOT}/${pruned_name}.sha256"
  echo "recovery-export: pruned an expired generation"
done < "${PRUNE_PLAN}"

echo "recovery-export: published ${BUNDLE_NAME}.tar.gz (${ARCHIVE_SIZE} bytes, sha256 ${ARCHIVE_SHA256})"
