#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
Usage:
  prepare-vapor-release-bundle.sh ARCHIVE VERSION CHANNEL PLATFORM MINIMUM_UPDATER_VERSION KEY_ID ED25519_PRIVATE_KEY OUTPUT_DIRECTORY [RELEASE_NOTES_URL [GIT_VERSION GIT_ARCHIVE GIT_SOURCE_BUNDLE GIT_NOTICES_BUNDLE]]

Prepares a signed, immutable Vapor release bundle on the operator's machine.
The private key is read locally and is never copied into the bundle.
EOF
}

if [ "$#" -ne 8 ] && [ "$#" -ne 9 ] && [ "$#" -ne 13 ]; then
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
git_version="${10:-}"
git_archive_input="${11:-}"
git_source_input="${12:-}"
git_notices_input="${13:-}"
public_origin="${VAPOR_RELEASE_PUBLIC_ORIGIN:-https://vapor.ghf-studios.site}"

safe_path_segment='^[A-Za-z0-9][A-Za-z0-9._+-]{0,99}$'
for value_name in release_version release_channel release_key_id; do
  value="${!value_name}"
  if [[ ! "$value" =~ $safe_path_segment ]]; then
    echo "error: $value_name must be a safe ASCII path/identifier segment" >&2
    exit 2
  fi
done

if [[ "$release_platform" != "windows-x86_64" && "$release_platform" != "linux-x86_64" ]]; then
  echo "error: supported release platforms are windows-x86_64 and linux-x86_64" >&2
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
if [ -n "$git_version" ]; then
  if [[ ! "$git_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$ ]]; then
    echo "error: GIT_VERSION must be a SemVer version" >&2
    exit 2
  fi
  for path in "$git_archive_input" "$git_source_input" "$git_notices_input"; do
    if [ ! -f "$path" ] || [ -L "$path" ]; then
      echo "error: all managed Git archive inputs must be regular files" >&2
      exit 1
    fi
  done
fi
if [ "$#" -eq 13 ] && [ -z "$git_version" ]; then
  echo "error: GIT_VERSION is required when managed Git asset inputs are supplied" >&2
  exit 2
fi

archive_path="$(cd -- "$(dirname -- "$archive_input")" && pwd)/$(basename -- "$archive_input")"
signing_key_path="$(cd -- "$(dirname -- "$signing_key_input")" && pwd)/$(basename -- "$signing_key_input")"
output_parent_input="$(dirname -- "$output_input")"
output_name="$(basename -- "$output_input")"
mkdir -p -- "$output_parent_input"
output_parent="$(cd -- "$output_parent_input" && pwd)"
output_directory="$output_parent/$output_name"
git_archive_path=""
git_source_path=""
git_notices_path=""
if [ -n "$git_version" ]; then
  git_archive_path="$(cd -- "$(dirname -- "$git_archive_input")" && pwd)/$(basename -- "$git_archive_input")"
  git_source_path="$(cd -- "$(dirname -- "$git_source_input")" && pwd)/$(basename -- "$git_source_input")"
  git_notices_path="$(cd -- "$(dirname -- "$git_notices_input")" && pwd)/$(basename -- "$git_notices_input")"
fi

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
if [ -n "$git_version" ]; then
  install -m 0644 -- "$git_archive_path" "$release_directory/git.tar.gz"
  install -m 0644 -- "$git_source_path" "$release_directory/git-source.tar.gz"
  install -m 0644 -- "$git_notices_path" "$release_directory/git-notices.tar.gz"
fi
case "$release_platform" in
  windows-x86_64) setup_name=vapor-setup.exe ;;
  linux-x86_64) setup_name=vapor-setup ;;
esac

export VAPOR_RELEASE_ARCHIVE="$release_directory/vapor.tar.gz"
export VAPOR_RELEASE_SETUP="$release_directory/$setup_name"
export VAPOR_RELEASE_VERSION="$release_version"
export VAPOR_RELEASE_CHANNEL="$release_channel"
export VAPOR_RELEASE_PLATFORM="$release_platform"
export VAPOR_RELEASE_MINIMUM_UPDATER="$minimum_updater_version"
export VAPOR_RELEASE_KEY_ID="$release_key_id"
export VAPOR_RELEASE_NOTES_URL="$release_notes_url"
export VAPOR_RELEASE_PUBLIC_ORIGIN="$public_origin"
export VAPOR_RELEASE_MANIFEST="$release_directory/manifest.json"
export VAPOR_RELEASE_GIT_VERSION="$git_version"
export VAPOR_RELEASE_GIT_ARTIFACT="$release_directory/git.tar.gz"
export VAPOR_RELEASE_GIT_SOURCE="$release_directory/git-source.tar.gz"
export VAPOR_RELEASE_GIT_NOTICES="$release_directory/git-notices.tar.gz"

python3 - <<'PY'
import hashlib
import json
import os
import re
import sys
import tarfile
import tomllib
from pathlib import PurePosixPath
from urllib.parse import urlparse

numeric = r"(?:0|[1-9][0-9]*)"
pre_id = r"(?:0|[1-9][0-9]*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*)"
build_id = r"[0-9A-Za-z-]+"
semver = re.compile(rf"^{numeric}\.{numeric}\.{numeric}(?:-{pre_id}(?:\.{pre_id})*)?(?:\+{build_id}(?:\.{build_id})*)?$")
segment = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._+-]{0,99}$")

version = os.environ["VAPOR_RELEASE_VERSION"]
minimum = os.environ["VAPOR_RELEASE_MINIMUM_UPDATER"]
git_version = os.environ["VAPOR_RELEASE_GIT_VERSION"]
channel = os.environ["VAPOR_RELEASE_CHANNEL"]
key_id = os.environ["VAPOR_RELEASE_KEY_ID"]
platform = os.environ["VAPOR_RELEASE_PLATFORM"]
origin = os.environ["VAPOR_RELEASE_PUBLIC_ORIGIN"].rstrip("/")
notes_url = os.environ["VAPOR_RELEASE_NOTES_URL"]
archive_path = os.environ["VAPOR_RELEASE_ARCHIVE"]

if not semver.fullmatch(version) or not semver.fullmatch(minimum):
    sys.exit("error: release and minimum-updater versions must be valid SemVer")
if git_version and not semver.fullmatch(git_version):
    sys.exit("error: managed Git version must be valid SemVer")
if git_version and ("-" in git_version or "+" in git_version):
    sys.exit("error: managed Git version must be a stable SemVer release")
if platform not in {"windows-x86_64", "linux-x86_64"}:
    sys.exit("error: unsupported release platform")
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
archive_paths = set()
app_manifest = None
vapor_home_manifest = None
setup_member = None
setup_digest = hashlib.sha256()
setup_size = 0
target = {
    "windows-x86_64": "x86_64-pc-windows-gnullvm",
    "linux-x86_64": "x86_64-unknown-linux-gnu",
}[platform]
required_runtime = {
    "windows-x86_64": [
        "vapor-home.toml",
        f"bin/{target}/vapor-entrypoint.exe",
        f"bin/{target}/vapor.exe",
        f"bin/{target}/vapor-installer.exe",
        f"bin/{target}/vapor-setup.exe",
        f"bin/{target}/vapor-gui.exe",
        f"bin/{target}/vapor-client-service.exe",
        f"bin/{target}/vapor-ide-host.exe",
        f"bin/{target}/steam_api64.dll",
        f"bin/{target}/libunwind.dll",
    ],
    "linux-x86_64": [
        "vapor-home.toml",
        f"bin/{target}/vapor-entrypoint",
        f"bin/{target}/vapor",
        f"bin/{target}/vapor-installer",
        f"bin/{target}/vapor-setup",
        f"bin/{target}/vapor-gui",
        f"bin/{target}/vapor-client-service",
        f"bin/{target}/vapor-ide-host",
        f"bin/{target}/libsteam_api.so",
    ],
}[platform]
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
                or any(part.endswith((".", " ")) or ":" in part for part in path.parts)
            ):
                sys.exit(f"error: unsafe Windows archive path: {name!r}")
            if not (member.isfile() or member.isdir()):
                sys.exit(f"error: unsupported archive entry type: {name!r}")
            folded_parts = {part.casefold() for part in path.parts}
            if folded_parts.intersection({"madmax", "mad max", "madmaxmodding", "steamapps"}) or path.name.casefold() in {
                "madmax.exe",
                "madmax_dx11.exe",
                "madmaxmodding.asi",
            }:
                sys.exit(f"error: release archive appears to contain Mad Max game files: {name!r}")
            normalized = "/".join(part.casefold() for part in path.parts if part not in ("", "."))
            if normalized in {"content/default", "content/installed", "content/packages"} or normalized.startswith(("content/default/", "content/installed/", "content/packages/")):
                sys.exit(f"error: release archive contains Steam/game payload: {name!r}")
            if normalized:
                if normalized in archive_paths:
                    sys.exit(f"error: release archive contains duplicate or case-colliding paths: {name!r}")
                archive_paths.add(normalized)
            if normalized == "app.vapor.toml":
                if not member.isfile() or member.size > 64 * 1024:
                    sys.exit("error: App.vapor.toml must be a regular file no larger than 64 KiB")
                stream = package.extractfile(member)
                app_manifest = stream.read(64 * 1024 + 1).decode("utf-8") if stream else None
            if normalized == "vapor-home.toml":
                if not member.isfile() or member.size > 64 * 1024:
                    sys.exit("error: Vapor-Home.toml must be a regular file no larger than 64 KiB")
                stream = package.extractfile(member)
                vapor_home_manifest = stream.read(64 * 1024 + 1).decode("utf-8") if stream else None
            setup_relative = f"bin/{target}/{os.path.basename(os.environ['VAPOR_RELEASE_SETUP'])}".casefold()
            if normalized == setup_relative:
                if not member.isfile() or member.size == 0 or member.size > 512 * 1024 * 1024:
                    sys.exit("error: standalone setup binary is not a regular bounded file")
                setup_stream = package.extractfile(member)
                if setup_stream is None:
                    sys.exit("error: could not read the standalone setup executable from the archive")
                with open(os.environ["VAPOR_RELEASE_SETUP"], "wb") as setup_output:
                    for block in iter(lambda: setup_stream.read(1024 * 1024), b""):
                        setup_size += len(block)
                        setup_digest.update(block)
                        setup_output.write(block)
                if setup_size != member.size:
                    sys.exit("error: extracted standalone setup size does not match its archive entry")
                setup_member = member
            unpacked_size += member.size
            if unpacked_size > 8 * 1024 * 1024 * 1024:
                sys.exit("error: release archive expands beyond the 8 GiB package limit")
except (OSError, tarfile.TarError) as error:
    sys.exit(f"error: release must be a readable gzip tar archive: {error}")
if member_count == 0:
    sys.exit("error: release archive contains no files")
if app_manifest is None:
    sys.exit("error: archive must contain the dedicated standalone App.vapor.toml")
if vapor_home_manifest is None:
    sys.exit("error: archive must contain Vapor-Home.toml so installed Vapor commands can resolve their home")
try:
    home_values = tomllib.loads(vapor_home_manifest)
except (tomllib.TOMLDecodeError, UnicodeError) as error:
    sys.exit(f"error: Vapor-Home.toml is invalid: {error}")
if home_values.get("schema") != 1 or home_values.get("vapor", {}).get("id") != "ghf-studios/vapor":
    sys.exit("error: Vapor-Home.toml does not identify the supported Vapor home")
section = None
root_values = {}
for line in app_manifest.splitlines():
    line = line.split("#", 1)[0].strip()
    if line.startswith("[") and line.endswith("]"):
        section = line.strip("[]")
        if section in {"root.steam", "root.content"}:
            sys.exit("error: standalone App.vapor.toml must not contain Steam or game content metadata")
        continue
    if section == "root" and "=" in line:
        key, value = line.split("=", 1)
        root_values[key.strip()] = value.strip().strip('"')
if root_values.get("name") != "vapor-client" or root_values.get("organization") != "ghf-studios":
    sys.exit("error: App.vapor.toml does not identify the Vapor standalone product")
missing_runtime = sorted(set(required_runtime) - archive_paths)
if missing_runtime:
    sys.exit("error: release archive is missing required runtime files: " + ", ".join(missing_runtime))
if setup_member is None:
    sys.exit("error: release archive is missing the standalone setup executable")
if setup_size == 0:
    sys.exit("error: extracted standalone setup executable is empty")

manifest = {
    "schema": 2 if git_version else 1,
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
    "setup": {
        "url": f"{origin}/api/updates/v1/vapor/releases/{version}/{platform}/{os.path.basename(os.environ['VAPOR_RELEASE_SETUP'])}",
        "size": setup_size,
        "sha256": setup_digest.hexdigest(),
    },
    "release_notes_url": notes_url,
}
if git_version:
    def digest_file(path, maximum_size):
        digest = hashlib.sha256()
        size = 0
        with open(path, "rb") as source:
            for block in iter(lambda: source.read(1024 * 1024), b""):
                size += len(block)
                if size > maximum_size:
                    sys.exit(f"error: managed Git asset exceeds its size limit: {path}")
                digest.update(block)
        if size == 0:
            sys.exit(f"error: managed Git asset is empty: {path}")
        return {"size": size, "sha256": digest.hexdigest()}

    base = f"{origin}/api/updates/v1/vapor/releases/{version}/{platform}"
    managed_git = {"version": git_version, "platform": platform}
    for field, env_name, file_name, maximum_size in (
        ("artifact", "VAPOR_RELEASE_GIT_ARTIFACT", "git.tar.gz", 2 * 1024**3),
        ("source", "VAPOR_RELEASE_GIT_SOURCE", "git-source.tar.gz", 8 * 1024**3),
        ("notices", "VAPOR_RELEASE_GIT_NOTICES", "git-notices.tar.gz", 64 * 1024**2),
    ):
        record = digest_file(os.environ[env_name], maximum_size)
        record["url"] = f"{base}/{file_name}"
        managed_git[field] = record
    manifest["managed_git"] = managed_git
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
