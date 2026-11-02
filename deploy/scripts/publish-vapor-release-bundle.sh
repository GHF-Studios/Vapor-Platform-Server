#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
Usage:
  publish-vapor-release-bundle.sh BUNDLE_DIRECTORY ED25519_PUBLIC_KEY EXPECTED_KEY_ID

Publishes one previously prepared bundle into VAPOR_UPDATES_ROOT. It refuses
to overwrite an immutable version and updates the channel pointer last.
EOF
}

if [ "$#" -ne 3 ]; then
  usage
  exit 2
fi

bundle_input="$1"
public_key_input="$2"
expected_key_id="$3"
updates_root_input="${VAPOR_UPDATES_ROOT:-/var/lib/vapor-server/updates}"
public_origin="${VAPOR_RELEASE_PUBLIC_ORIGIN:-https://vapor.ghf-studios.site}"
if [ "$public_origin" != "https://vapor.ghf-studios.site" ]; then
  echo "error: client trust currently accepts only https://vapor.ghf-studios.site" >&2
  exit 1
fi

for command_name in openssl python3; do
  if ! command -v "$command_name" >/dev/null 2>&1; then
    echo "error: required command is unavailable: $command_name" >&2
    exit 1
  fi
done
if [ ! -d "$bundle_input" ] || [ ! -f "$public_key_input" ]; then
  echo "error: bundle directory and public verification key must exist" >&2
  exit 1
fi

bundle_directory="$(cd -- "$bundle_input" && pwd -P)"
public_key_path="$(cd -- "$(dirname -- "$public_key_input")" && pwd -P)/$(basename -- "$public_key_input")"
mkdir -p -- "$updates_root_input"
updates_root="$(cd -- "$updates_root_input" && pwd -P)"
assert_managed_directory() {
  local path_input="$1"
  local canonical
  canonical="$(cd -- "$path_input" && pwd -P)" || {
    echo "error: cannot resolve managed release directory: $path_input" >&2
    exit 1
  }
  case "$canonical" in
    "$updates_root"/*) ;;
    *)
      echo "error: managed release path escapes VAPOR_UPDATES_ROOT: $canonical" >&2
      exit 1
      ;;
  esac
}
export VAPOR_BUNDLE_DIRECTORY="$bundle_directory"
export VAPOR_PUBLIC_KEY_PATH="$public_key_path"
export VAPOR_UPDATES_ROOT_CANONICAL="$updates_root"
export VAPOR_RELEASE_PUBLIC_ORIGIN="$public_origin"
export VAPOR_EXPECTED_KEY_ID="$expected_key_id"

mapfile -d '' pointer_paths < <(find "$bundle_directory/channels" -type f -name '*.version' -print0 2>/dev/null)
if [ "${#pointer_paths[@]}" -ne 1 ]; then
  echo "error: bundle must contain exactly one channel pointer" >&2
  exit 1
fi
pointer_path="${pointer_paths[0]}"
channel="$(basename -- "$(dirname -- "$pointer_path")")"
platform="$(basename -- "$pointer_path")"
platform="${platform%.version}"
version="$(tr -d '\r\n' < "$pointer_path")"

safe_path_segment='^[A-Za-z0-9][A-Za-z0-9._+-]{0,99}$'
for value_name in channel platform version expected_key_id; do
  value="${!value_name}"
  if [[ ! "$value" =~ $safe_path_segment ]]; then
    echo "error: $value_name in release bundle is not a safe path/identifier segment" >&2
    exit 1
  fi
done

export VAPOR_RELEASE_CHANNEL="$channel"
export VAPOR_RELEASE_PLATFORM="$platform"
export VAPOR_RELEASE_VERSION="$version"

release_directory="$bundle_directory/releases/$version/$platform"
manifest_path="$release_directory/manifest.json"
signature_path="$release_directory/manifest.json.sig"
archive_path="$release_directory/vapor.tar.gz"
if [ ! -f "$manifest_path" ] || [ ! -f "$signature_path" ] || [ ! -f "$archive_path" ]; then
  echo "error: bundle is missing its manifest, signature, or archive" >&2
  exit 1
fi

python3 - <<'PY'
import hashlib
import json
import os
import re
import sys
from urllib.parse import urlparse

numeric = r"(?:0|[1-9][0-9]*)"
pre_id = r"(?:0|[1-9][0-9]*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*)"
build_id = r"[0-9A-Za-z-]+"
semver = re.compile(rf"^{numeric}\.{numeric}\.{numeric}(?:-{pre_id}(?:\.{pre_id})*)?(?:\+{build_id}(?:\.{build_id})*)?$")
segment = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._+-]{0,99}$")
bundle = os.environ["VAPOR_BUNDLE_DIRECTORY"]
updates = os.environ["VAPOR_UPDATES_ROOT_CANONICAL"]
if os.path.commonpath([bundle, updates]) in (bundle, updates):
    sys.exit("error: release bundle and live update root must not overlap")
channel = os.environ["VAPOR_RELEASE_CHANNEL"]
platform = os.environ["VAPOR_RELEASE_PLATFORM"]
version = os.environ["VAPOR_RELEASE_VERSION"]
expected_key_id = os.environ["VAPOR_EXPECTED_KEY_ID"]
if not all(segment.fullmatch(value) for value in (channel, platform)) or not semver.fullmatch(version):
    sys.exit("error: channel/platform/version in the bundle path is invalid")
if not segment.fullmatch(expected_key_id):
    sys.exit("error: expected key id is invalid")
directory = os.path.join(bundle, "releases", version, platform)
manifest_path = os.path.join(directory, "manifest.json")
signature_path = os.path.join(directory, "manifest.json.sig")
archive_path = os.path.join(directory, "vapor.tar.gz")
with open(manifest_path, "rb") as manifest_file:
    raw_manifest = manifest_file.read(64 * 1024 + 1)
if len(raw_manifest) > 64 * 1024:
    sys.exit("error: manifest exceeds 64 KiB")
try:
    manifest = json.loads(raw_manifest)
except (UnicodeDecodeError, json.JSONDecodeError):
    sys.exit("error: manifest is not valid JSON")
if not isinstance(manifest, dict):
    sys.exit("error: manifest root must be a JSON object")
if set(manifest) != {
    "schema",
    "key_id",
    "version",
    "channel",
    "platform",
    "minimum_updater_version",
    "artifact",
    "release_notes_url",
}:
    sys.exit("error: manifest fields do not match the supported release schema")
if type(manifest.get("schema")) is not int or manifest["schema"] != 1:
    sys.exit("error: unsupported manifest schema")
if not isinstance(manifest.get("version"), str) or manifest["version"] != version:
    sys.exit("error: manifest schema/version does not match its bundle path")
if not isinstance(manifest.get("channel"), str) or manifest["channel"] != channel:
    sys.exit("error: manifest channel does not match its bundle path")
if not isinstance(manifest.get("platform"), str) or manifest["platform"] != platform:
    sys.exit("error: manifest channel/platform does not match its bundle path")
if not isinstance(manifest.get("key_id"), str) or manifest["key_id"] != expected_key_id:
    sys.exit("error: manifest key id does not match the selected trusted key")
minimum_version = manifest.get("minimum_updater_version")
if not isinstance(minimum_version, str) or not semver.fullmatch(minimum_version):
    sys.exit("error: manifest minimum_updater_version is not valid SemVer")
notes = manifest.get("release_notes_url")
try:
    parsed_notes = urlparse(notes) if isinstance(notes, str) else None
    notes_port = parsed_notes.port if parsed_notes is not None else None
except ValueError:
    parsed_notes = None
    notes_port = None
if (
    parsed_notes is None
    or parsed_notes.scheme != "https"
    or parsed_notes.hostname != "vapor.ghf-studios.site"
    or parsed_notes.username
    or parsed_notes.password
    or notes_port is not None
    or parsed_notes.query
    or parsed_notes.fragment
):
    sys.exit("error: release_notes_url must use the Vapor HTTPS origin without query/fragment")
artifact = manifest.get("artifact")
if not isinstance(artifact, dict):
    sys.exit("error: manifest artifact record is missing")
if set(artifact) != {"url", "size", "sha256"}:
    sys.exit("error: manifest artifact fields are invalid")
expected_url = f"https://vapor.ghf-studios.site/api/updates/v1/vapor/releases/{version}/{platform}/vapor.tar.gz"
if not isinstance(artifact.get("url"), str) or artifact["url"] != expected_url:
    sys.exit("error: artifact URL does not match the configured public endpoint")
digest = hashlib.sha256()
size = 0
with open(archive_path, "rb") as archive:
    for block in iter(lambda: archive.read(1024 * 1024), b""):
        size += len(block)
        digest.update(block)
if (
    size == 0
    or type(artifact.get("size")) is not int
    or artifact["size"] != size
    or not isinstance(artifact.get("sha256"), str)
    or artifact["sha256"] != digest.hexdigest()
):
    sys.exit("error: archive size/hash does not match the signed manifest")
with open(signature_path, "rb") as signature_file:
    signature_hex = signature_file.read(257)
if not re.fullmatch(rb"[0-9a-f]{128}", signature_hex):
    sys.exit("error: signature must be 128 lowercase hexadecimal characters")
PY

signature_raw="$(mktemp)"
stage_directory=""
pointer_temp=""
cleanup() {
  rm -f -- "$signature_raw"
  if [ -n "$pointer_temp" ]; then
    rm -f -- "$pointer_temp"
  fi
  if [ -n "$stage_directory" ] && [ -d "$stage_directory" ]; then
    rm -rf -- "$stage_directory"
  fi
}
trap cleanup EXIT
python3 - "$signature_path" "$signature_raw" <<'PY'
import sys
with open(sys.argv[1], "rb") as source:
    value = source.read().strip()
with open(sys.argv[2], "wb") as destination:
    destination.write(bytes.fromhex(value.decode("ascii")))
PY
openssl pkeyutl -verify -pubin -inkey "$public_key_path" -rawin \
  -in "$manifest_path" -sigfile "$signature_raw" >/dev/null

final_release_directory="$updates_root/releases/$version/$platform"
if [ -e "$final_release_directory" ]; then
  echo "error: immutable release already exists: $final_release_directory" >&2
  exit 1
fi

mkdir -p -- "$updates_root/releases" "$updates_root/channels"
assert_managed_directory "$updates_root/releases"
assert_managed_directory "$updates_root/channels"
mkdir -p -- "$updates_root/releases/$version" "$updates_root/channels/$channel"
assert_managed_directory "$updates_root/releases/$version"
assert_managed_directory "$updates_root/channels/$channel"
chmod 0755 "$updates_root/releases" "$updates_root/releases/$version" \
  "$updates_root/channels" "$updates_root/channels/$channel"
stage_directory="$(mktemp -d "$updates_root/.incoming.XXXXXX")"
mkdir -p -- "$stage_directory/releases/$version"
install -m 0644 -- "$manifest_path" "$stage_directory/releases/$version/$platform.manifest.json"
install -m 0644 -- "$signature_path" "$stage_directory/releases/$version/$platform.manifest.json.sig"
install -m 0644 -- "$archive_path" "$stage_directory/releases/$version/$platform.vapor.tar.gz"

release_stage="$stage_directory/releases/$version/$platform"
mkdir -p -- "$release_stage"
chmod 0755 "$release_stage"
mv -- "$stage_directory/releases/$version/$platform.manifest.json" "$release_stage/manifest.json"
mv -- "$stage_directory/releases/$version/$platform.manifest.json.sig" "$release_stage/manifest.json.sig"
mv -- "$stage_directory/releases/$version/$platform.vapor.tar.gz" "$release_stage/vapor.tar.gz"
mv -T -- "$release_stage" "$final_release_directory"

pointer_directory="$updates_root/channels/$channel"
pointer_temp="$(mktemp "$pointer_directory/.${platform}.version.XXXXXX")"
printf '%s\n' "$version" > "$pointer_temp"
chmod 0644 "$pointer_temp"
mv -f -- "$pointer_temp" "$pointer_directory/$platform.version"
pointer_temp=""

rm -rf -- "$stage_directory"
stage_directory=""
echo "Published Vapor $version ($channel/$platform) to $updates_root"
