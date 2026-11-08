#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "${SCRIPT_DIR}/lib.sh"

require_root

BACKUP_ROOT="${VAPOR_STATE_ONLY_BACKUP_ROOT:-${VAPOR_BACKUP_ROOT:-/var/backups/vapor-server}/state-only}"
KEEP="${VAPOR_STATE_ONLY_RETENTION_COUNT:-168}"
if [[ ! "${KEEP}" =~ ^[0-9]+$ ]] || [ "${KEEP}" -lt 2 ]; then
  echo "error: VAPOR_STATE_ONLY_RETENTION_COUNT must be an integer of at least 2" >&2
  exit 1
fi
if ! command -v flock >/dev/null 2>&1; then
  echo "error: flock is required for state export locking" >&2
  exit 1
fi

STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
NONCE="$(openssl rand -hex 6)"
BUNDLE_NAME="vapor-platform-state-${STAMP}-${NONCE}.tar.gz"
install_dir root root 0700 "${BACKUP_ROOT}"
if [ -e "${BACKUP_ROOT}/${BUNDLE_NAME}" ] || [ -e "${BACKUP_ROOT}/${BUNDLE_NAME}.sha256" ]; then
  echo "error: state export target already exists" >&2
  exit 1
fi

exec 9>/run/vapor-server-deploy.lock
if ! flock -n 9; then
  echo "error: Vapor deploy/state lock is already held; retry the export" >&2
  exit 1
fi

TMP_DIR="$(mktemp -d "${BACKUP_ROOT}/.state-export.XXXXXX")"
BUNDLE_ROOT="${TMP_DIR}/vapor-platform-recovery"
mkdir -m 0700 -p "${BUNDLE_ROOT}/state"
cleanup() {
  local status=$?
  trap - EXIT
  rm -rf -- "${TMP_DIR}"
  exit "${status}"
}
trap cleanup EXIT

copy_optional() {
  local relative="$1"
  local source="${VAPOR_STATE_ROOT}/${relative}"
  if [ -e "${source}" ] || [ -L "${source}" ]; then
    [ -f "${source}" ] && [ ! -L "${source}" ] || {
      echo "error: optional state file is not a regular file: ${relative}" >&2
      return 1
    }
    install -D -o root -g root -m 0600 "${source}" "${BUNDLE_ROOT}/state/${relative}"
  fi
}
copy_tree() {
  local relative="$1"
  local source="${VAPOR_STATE_ROOT}/${relative}"
  if [ -d "${source}" ] && [ ! -L "${source}" ]; then
    if [ -n "$(find "${source}" \( -type l -o \( ! -type d -a ! -type f \) \) -print -quit)" ]; then
      echo "error: state tree contains a link or special file: ${relative}" >&2
      return 1
    fi
    mkdir -p "${BUNDLE_ROOT}/state/${relative}"
    cp -a "${source}/." "${BUNDLE_ROOT}/state/${relative}/"
  elif [ -e "${source}" ] || [ -L "${source}" ]; then
    echo "error: state tree root is not a regular directory: ${relative}" >&2
    return 1
  fi
}

python3 "${SCRIPT_DIR}/snapshot-state-databases.py" "${VAPOR_STATE_ROOT}" "${BUNDLE_ROOT}/state"
copy_optional operations/deployed-sources.json
copy_tree docs/current
copy_tree docs/releases
copy_tree updates/releases
python3 "${SCRIPT_DIR}/validate-state-databases.py" "${BUNDLE_ROOT}/state"

CREATED="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
python3 - "${BUNDLE_ROOT}/manifest.toml" "${CREATED}" <<'PY'
import json
import sys

path, created_at = sys.argv[1:]
manifest = {
    "schema_version": 3,
    "created_at_utc": created_at,
    "state_catalog_version": 1,
    "secrets_included": False,
    "archive_encryption": "none",
    "build_outputs_excluded": True,
    "config_included": False,
    "host_routing_included": False,
    "source_pins_included": False,
    "recovery_kit_included": False,
    "included_categories": ["state"],
}
with open(path, "w", encoding="utf-8", newline="\n") as output:
    for key, value in manifest.items():
        if isinstance(value, bool):
            encoded = "true" if value else "false"
        elif isinstance(value, int):
            encoded = str(value)
        else:
            encoded = json.dumps(value, ensure_ascii=True)
        output.write(f"{key} = {encoded}\n")
PY

ARCHIVE_TMP="${TMP_DIR}/${BUNDLE_NAME}"
tar -C "${TMP_DIR}" -czf "${ARCHIVE_TMP}" vapor-platform-recovery
python3 "${SCRIPT_DIR}/state-bundle-validate.py" "${ARCHIVE_TMP}"
DIGEST="$(sha256sum "${ARCHIVE_TMP}" | awk '{print $1}')"
SIZE="$(stat -c '%s' "${ARCHIVE_TMP}")"
mv -- "${ARCHIVE_TMP}" "${BACKUP_ROOT}/${BUNDLE_NAME}"
printf '%s  %s\n' "${DIGEST}" "${BUNDLE_NAME}" > "${TMP_DIR}/archive.sha256"
chmod 0600 "${BACKUP_ROOT}/${BUNDLE_NAME}" "${TMP_DIR}/archive.sha256"
mv -- "${TMP_DIR}/archive.sha256" "${BACKUP_ROOT}/${BUNDLE_NAME}.sha256"

python3 - "${BACKUP_ROOT}" "${BUNDLE_NAME}" "${DIGEST}" "${SIZE}" "${CREATED}" "${KEEP}" <<'PY'
import datetime
import hashlib
import json
import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1])
incoming, digest, size, created, keep = sys.argv[2], sys.argv[3], int(sys.argv[4]), sys.argv[5], int(sys.argv[6])
pattern = re.compile(r"vapor-platform-state-\d{8}T\d{6}Z-[0-9a-f]{12}\.tar\.gz\Z")
records = []
for archive in sorted(root.iterdir()):
    if not pattern.fullmatch(archive.name) or archive.is_symlink() or not archive.is_file():
        continue
    sidecar = pathlib.Path(f"{archive}.sha256")
    try:
        recorded_digest, recorded_name = sidecar.read_text(encoding="ascii").strip().split(maxsplit=1)
    except (OSError, ValueError, UnicodeError):
        continue
    if recorded_name != archive.name or not re.fullmatch(r"[0-9a-f]{64}", recorded_digest):
        continue
    actual_hash = hashlib.sha256()
    with archive.open("rb") as contents:
        for chunk in iter(lambda: contents.read(1024 * 1024), b""):
            actual_hash.update(chunk)
    actual = actual_hash.hexdigest()
    if actual != recorded_digest:
        continue
    created_at = datetime.datetime.strptime(archive.name.split("-")[3], "%Y%m%dT%H%M%SZ").replace(tzinfo=datetime.timezone.utc)
    records.append({"archive": archive.name, "sha256": actual, "size_bytes": archive.stat().st_size,
                    "created_at_utc": created_at.strftime("%Y-%m-%dT%H:%M:%SZ")})
if incoming not in {record["archive"] for record in records}:
    raise SystemExit("state-export: completed archive is missing from its verified index")
records = sorted(records, key=lambda item: item["archive"])[-keep:]
index_tmp = root / ".state-index.json.tmp"
latest_tmp = root / ".state-latest.json.tmp"
index_tmp.write_text(json.dumps({"schema_version": 1, "archives": records}, separators=(",", ":")) + "\n", encoding="utf-8")
latest_tmp.write_text(json.dumps({"schema_version": 1, "archive": incoming, "sha256": digest,
                                 "size_bytes": size, "created_at_utc": created,
                                 "archive_schema_version": 3}, separators=(",", ":")) + "\n", encoding="utf-8")
index_tmp.chmod(0o600)
latest_tmp.chmod(0o600)
index_tmp.replace(root / "index.json")
latest_tmp.replace(root / "latest.json")
for archive in root.glob("vapor-platform-state-*.tar.gz"):
    if archive.name not in {record["archive"] for record in records}:
        archive.unlink()
        pathlib.Path(f"{archive}.sha256").unlink(missing_ok=True)
PY

echo "state-export: published ${BUNDLE_NAME} (${SIZE} bytes, sha256 ${DIGEST})"
