#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
Usage:
  prepare-vapor-release-bundle.sh ARCHIVE VERSION CHANNEL PLATFORM MINIMUM_UPDATER_VERSION KEY_ID ED25519_PRIVATE_KEY OUTPUT_DIRECTORY [RELEASE_NOTES_URL]

Prepares a signed, immutable Vapor release bundle on the operator's machine.
The private key is read locally and is never copied into the bundle.
EOF
}

if [ "$#" -lt 8 ] || [ "$#" -gt 9 ]; then
  usage
  exit 2
fi

archive_input="$1"
release_version="$2"
release_channel="$3"
release_platform="$4"
minimum_updater_version="$5"
release_key_id="$6"
signing_key_input="$7"
output_input="$8"
release_notes_url="${9:-https://vapor.ghf-studios.site/docs/}"
public_origin="${VAPOR_RELEASE_PUBLIC_ORIGIN:-https://vapor.ghf-studios.site}"

safe_path_segment='^[A-Za-z0-9][A-Za-z0-9._+-]{0,99}$'
for value_name in release_version release_channel release_key_id; do
  value="${!value_name}"
  if [[ ! "$value" =~ $safe_path_segment ]]; then
    echo "error: $value_name must be a safe ASCII path/identifier segment" >&2
    exit 2
  fi
done

if [ "$release_platform" != "windows-x86_64" ]; then
  echo "error: this release pipeline currently supports only windows-x86_64" >&2
  exit 2
fi

for command_name in openssl python3 od tr; do
  if ! command -v "$command_name" >/dev/null 2>&1; then
    echo "error: required command is unavailable: $command_name" >&2
    exit 1
  fi
done

if [ ! -f "$archive_input" ] || [ ! -f "$signing_key_input" ]; then
  echo "error: archive and signing key must both be regular files" >&2
  exit 1
fi

archive_path="$(cd -- "$(dirname -- "$archive_input")" && pwd)/$(basename -- "$archive_input")"
signing_key_path="$(cd -- "$(dirname -- "$signing_key_input")" && pwd)/$(basename -- "$signing_key_input")"
output_parent_input="$(dirname -- "$output_input")"
output_name="$(basename -- "$output_input")"
mkdir -p -- "$output_parent_input"
output_parent="$(cd -- "$output_parent_input" && pwd)"
output_directory="$output_parent/$output_name"

if [ -e "$output_directory" ]; then
  echo "error: output already exists; immutable bundles are never overwritten: $output_directory" >&2
  exit 1
fi

bundle_temp="$(mktemp -d "$output_parent/.vapor-release.XXXXXX")"
cleanup() {
  if [ -n "${bundle_temp:-}" ] && [ -d "$bundle_temp" ]; then
    rm -rf -- "$bundle_temp"
  fi
}
trap cleanup EXIT

release_directory="$bundle_temp/releases/$release_version/$release_platform"
mkdir -p -- "$release_directory" "$bundle_temp/channels/$release_channel"
install -m 0644 -- "$archive_path" "$release_directory/vapor.tar.gz"

export VAPOR_RELEASE_ARCHIVE="$release_directory/vapor.tar.gz"
export VAPOR_RELEASE_VERSION="$release_version"
export VAPOR_RELEASE_CHANNEL="$release_channel"
export VAPOR_RELEASE_PLATFORM="$release_platform"
export VAPOR_RELEASE_MINIMUM_UPDATER="$minimum_updater_version"
export VAPOR_RELEASE_KEY_ID="$release_key_id"
export VAPOR_RELEASE_NOTES_URL="$release_notes_url"
export VAPOR_RELEASE_PUBLIC_ORIGIN="$public_origin"
export VAPOR_RELEASE_MANIFEST="$release_directory/manifest.json"

python3 - <<'PY'
import hashlib
import json
import os
import re
import sys
import tarfile
from pathlib import PurePosixPath
from urllib.parse import urlparse

numeric = r"(?:0|[1-9][0-9]*)"
pre_id = r"(?:0|[1-9][0-9]*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*)"
build_id = r"[0-9A-Za-z-]+"
semver = re.compile(rf"^{numeric}\.{numeric}\.{numeric}(?:-{pre_id}(?:\.{pre_id})*)?(?:\+{build_id}(?:\.{build_id})*)?$")
segment = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._+-]{0,99}$")

version = os.environ["VAPOR_RELEASE_VERSION"]
minimum = os.environ["VAPOR_RELEASE_MINIMUM_UPDATER"]
channel = os.environ["VAPOR_RELEASE_CHANNEL"]
key_id = os.environ["VAPOR_RELEASE_KEY_ID"]
platform = os.environ["VAPOR_RELEASE_PLATFORM"]
origin = os.environ["VAPOR_RELEASE_PUBLIC_ORIGIN"].rstrip("/")
notes_url = os.environ["VAPOR_RELEASE_NOTES_URL"]
archive_path = os.environ["VAPOR_RELEASE_ARCHIVE"]

if not semver.fullmatch(version) or not semver.fullmatch(minimum):
    sys.exit("error: release and minimum-updater versions must be valid SemVer")
if not segment.fullmatch(channel) or not segment.fullmatch(key_id):
    sys.exit("error: channel and key id must be safe ASCII path/identifier values")
parsed_origin = urlparse(origin)
if (
    origin != "https://vapor.ghf-studios.site"
    or
    parsed_origin.scheme != "https"
    or not parsed_origin.netloc
    or parsed_origin.path
    or parsed_origin.query
    or parsed_origin.fragment
    or parsed_origin.username
    or parsed_origin.password
):
    sys.exit("error: public origin must be an HTTPS origin without a path")
if notes_url:
    try:
        parsed_notes = urlparse(notes_url)
        notes_port = parsed_notes.port
    except ValueError:
        sys.exit("error: release notes URL is invalid")
    if (
        parsed_notes.scheme != "https"
        or parsed_notes.hostname != "vapor.ghf-studios.site"
        or parsed_notes.username
        or parsed_notes.password
        or notes_port is not None
        or parsed_notes.query
        or parsed_notes.fragment
    ):
        sys.exit("error: release notes URL must use the Vapor HTTPS origin without query/fragment")

digest = hashlib.sha256()
size = 0
with open(archive_path, "rb") as archive:
    for block in iter(lambda: archive.read(1024 * 1024), b""):
        size += len(block)
        digest.update(block)
if size == 0:
    sys.exit("error: release archive is empty")

member_count = 0
unpacked_size = 0
try:
    with tarfile.open(archive_path, mode="r:gz") as package:
        for member in package:
            member_count += 1
            if member_count > 100_000:
                sys.exit("error: release archive contains too many entries")
            name = member.name
            path = PurePosixPath(name)
            if (
                not name
                or "\\" in name
                or path.is_absolute()
                or re.match(r"^[A-Za-z]:", name)
                or ".." in path.parts
            ):
                sys.exit(f"error: unsafe Windows archive path: {name!r}")
            if not (member.isfile() or member.isdir()):
                sys.exit(f"error: unsupported archive entry type: {name!r}")
            folded_parts = {part.casefold() for part in path.parts}
            if folded_parts.intersection({"madmax", "mad max", "madmaxmodding"}) or path.name.casefold() in {
                "madmax.exe",
                "madmax_dx11.exe",
                "madmaxmodding.asi",
            }:
                sys.exit(f"error: release archive appears to contain Mad Max game files: {name!r}")
            unpacked_size += member.size
            if unpacked_size > 8 * 1024 * 1024 * 1024:
                sys.exit("error: release archive expands beyond the 8 GiB package limit")
except (OSError, tarfile.TarError) as error:
    sys.exit(f"error: release must be a readable gzip tar archive: {error}")
if member_count == 0:
    sys.exit("error: release archive contains no files")

manifest = {
    "schema": 1,
    "key_id": key_id,
    "version": version,
    "channel": channel,
    "platform": platform,
    "minimum_updater_version": minimum,
    "artifact": {
        "url": f"{origin}/api/updates/v1/vapor/releases/{version}/{platform}/vapor.tar.gz",
        "size": size,
        "sha256": digest.hexdigest(),
    },
    "release_notes_url": notes_url,
}
encoded = (json.dumps(manifest, ensure_ascii=False, separators=(",", ":"), sort_keys=True) + "\n").encode()
with open(os.environ["VAPOR_RELEASE_MANIFEST"], "wb") as output:
    output.write(encoded)
PY

openssl pkeyutl -sign -rawin \
  -inkey "$signing_key_path" \
  -in "$release_directory/manifest.json" \
  -out "$bundle_temp/signature.raw"
openssl pkey -in "$signing_key_path" -pubout -out "$bundle_temp/signing-public.pem" >/dev/null
openssl pkeyutl -verify -pubin -inkey "$bundle_temp/signing-public.pem" -rawin \
  -in "$release_directory/manifest.json" \
  -sigfile "$bundle_temp/signature.raw" >/dev/null
od -An -vtx1 "$bundle_temp/signature.raw" | tr -d ' \n' > "$release_directory/manifest.json.sig"
rm -f -- "$bundle_temp/signature.raw" "$bundle_temp/signing-public.pem"
printf '%s\n' "$release_version" > "$bundle_temp/channels/$release_channel/$release_platform.version"
chmod 0644 "$release_directory/manifest.json" "$release_directory/manifest.json.sig" \
  "$bundle_temp/channels/$release_channel/$release_platform.version"

mv -T -- "$bundle_temp" "$output_directory"
bundle_temp=""
echo "Prepared signed Vapor release bundle: $output_directory"
