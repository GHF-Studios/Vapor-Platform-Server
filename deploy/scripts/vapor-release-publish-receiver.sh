#!/usr/bin/env bash
set -euo pipefail

# Forced-command receiver for the CI release key. The SSH client supplies one
# compressed bundle on stdin and only the platform selector as its command.
if [[ "$(id -u)" -ne 0 ]]; then
  echo "error: release receiver must run as root through its dedicated sudo rule" >&2
  exit 1
fi

case "${SSH_ORIGINAL_COMMAND:-}" in
  "publish linux-x86_64") platform=linux-x86_64 ;;
  "publish windows-x86_64") platform=windows-x86_64 ;;
  *) echo "error: allowed command is publish linux-x86_64 or publish windows-x86_64" >&2; exit 2 ;;
esac

checkout="${VAPOR_PLATFORM_CHECKOUT:-/opt/vapor-platform-server}"
publisher="$checkout/deploy/scripts/publish-vapor-release-bundle.sh"
public_key="${VAPOR_RELEASE_PUBLIC_KEY:-/etc/vapor-server/release-keys/vapor-release-2026-11-24.pem}"
key_id=vapor-release-2026-11-24
updates_root="${VAPOR_UPDATES_ROOT:-/var/lib/vapor-server/updates}"
export VAPOR_UPDATES_ROOT="$updates_root"
if [[ ! -x "$publisher" || ! -f "$public_key" ]]; then
  echo "error: deployed publisher or trusted release public key is unavailable" >&2
  exit 1
fi

incoming_root="${VAPOR_RELEASE_INCOMING_ROOT:-/var/tmp}"
stage="$(mktemp -d "$incoming_root/vapor-release-incoming.XXXXXX")"
chmod 0700 "$stage"
cleanup() { rm -rf -- "$stage"; }
trap cleanup EXIT

python3 - "$stage" "$platform" 3<&0 <<'PY'
import os
import re
import sys
import tarfile
from pathlib import Path, PurePosixPath

root = Path(sys.argv[1])
platform = sys.argv[2]
expected_setup = "vapor-setup.exe" if platform == "windows-x86_64" else "vapor-setup"
version_pattern = re.compile(r"^(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$")
files = set()
directories = set()
version = None
total = 0
max_total = 8 * 1024 * 1024 * 1024
max_entries = 32
entries = 0

try:
    # Keep the streamed upload on fd 3: stdin carries this embedded Python
    # program via the heredoc, so reading the archive from stdin loses it.
    archive = tarfile.open(fileobj=os.fdopen(3, "rb"), mode="r|gz")
except tarfile.TarError as error:
    sys.exit(f"error: incoming stream is not a valid gzip tar bundle: {error}")

with archive:
    for member in archive:
        entries += 1
        if entries > max_entries:
            sys.exit("error: incoming release bundle contains too many entries")
        name = member.name
        while name.startswith("./"):
            name = name[2:]
        if name in ("", ".") and member.isdir():
            continue
        if not name or "\\" in name:
            sys.exit("error: bundle contains an invalid path")
        path = PurePosixPath(name)
        if path.is_absolute() or any(part in ("", ".", "..") for part in path.parts):
            sys.exit(f"error: bundle contains an unsafe path: {member.name!r}")
        if not (member.isdir() or member.isfile()) or member.issym() or member.islnk():
            sys.exit(f"error: bundle contains a link or special file: {member.name!r}")
        normalized = "/".join(path.parts)
        if member.isdir():
            if normalized in directories:
                sys.exit(f"error: bundle repeats directory: {normalized}")
            directories.add(normalized)
            if normalized not in {"channels", "channels/stable", "releases"} and not re.fullmatch(r"releases/[^/]+(?:/[^/]+)?", normalized):
                sys.exit(f"error: unexpected bundle directory: {normalized}")
            continue
        if normalized in files:
            sys.exit(f"error: bundle repeats file: {normalized}")
        if normalized == f"channels/stable/{platform}.version":
            destination = root / normalized
        elif normalized.startswith("releases/"):
            parts = path.parts
            if len(parts) != 4 or parts[2] != platform or not version_pattern.fullmatch(parts[1]):
                sys.exit(f"error: bundle release path does not match {platform}: {normalized}")
            if version is not None and version != parts[1]:
                sys.exit("error: bundle contains more than one release version")
            version = parts[1]
            if parts[3] not in {"manifest.json", "manifest.json.sig", "vapor.tar.gz", expected_setup}:
                sys.exit(f"error: bundle contains an unexpected release file: {normalized}")
            destination = root / normalized
        else:
            sys.exit(f"error: bundle contains an unexpected file: {normalized}")
        max_file = 8 * 1024 * 1024 * 1024
        if normalized.endswith("manifest.json"):
            max_file = 64 * 1024
        elif normalized.endswith("manifest.json.sig"):
            max_file = 128
        elif normalized.endswith(".version"):
            max_file = 256
        elif normalized.endswith("vapor-setup") or normalized.endswith("vapor-setup.exe"):
            max_file = 256 * 1024 * 1024
        if member.size < 0 or member.size > max_file:
            sys.exit(f"error: bundle file size is outside policy: {normalized}")
        total += member.size
        if total > max_total:
            sys.exit("error: incoming release bundle exceeds the size limit")
        destination.parent.mkdir(parents=True, exist_ok=True)
        source = archive.extractfile(member)
        if source is None:
            sys.exit(f"error: could not read bundle file: {normalized}")
        remaining = member.size
        with source, destination.open("xb") as output:
            while remaining:
                block = source.read(min(1024 * 1024, remaining))
                if not block:
                    sys.exit(f"error: truncated bundle file: {normalized}")
                output.write(block)
                remaining -= len(block)
        os.chmod(destination, 0o600)
        files.add(normalized)

if version is None:
    sys.exit("error: release bundle contains no platform archive")
expected = {
    f"channels/stable/{platform}.version",
    f"releases/{version}/{platform}/manifest.json",
    f"releases/{version}/{platform}/manifest.json.sig",
    f"releases/{version}/{platform}/vapor.tar.gz",
    f"releases/{version}/{platform}/{expected_setup}",
}
if files != expected:
    sys.exit(f"error: bundle file set is incomplete or unexpected: {sorted(files ^ expected)}")
expected_directories = {
    "channels",
    "channels/stable",
    "releases",
    f"releases/{version}",
    f"releases/{version}/{platform}",
}
if directories != expected_directories:
    sys.exit(f"error: bundle directory set is incomplete or unexpected: {sorted(directories ^ expected_directories)}")
pointer = (root / f"channels/stable/{platform}.version").read_text().strip()
if pointer != version:
    sys.exit("error: stable pointer does not match the immutable release version")
PY

bash "$publisher" "$stage" "$public_key" "$key_id"
