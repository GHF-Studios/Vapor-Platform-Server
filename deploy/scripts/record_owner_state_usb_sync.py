#!/usr/bin/env python3
"""Atomically record bounded metadata for an owner-verified state-only USB sync."""

from __future__ import annotations

import argparse
import base64
import binascii
import datetime as dt
import json
import os
import re
import secrets
import stat
import sys
from pathlib import Path
from typing import Any


MAX_ENCODED_BYTES = 4096
MAX_GENERATIONS = 10_000
MAX_PAYLOAD_BYTES = 1 << 40
FIELDS = {
    "schema_version",
    "last_success_unix",
    "latest_archive",
    "latest_archive_sha256",
    "verified_generation_count",
    "retained_payload_bytes",
    "source_commit",
    "latest_rehashed",
    "recovery_kit_available",
}
ARCHIVE = re.compile(r"^vapor-platform-state-(\d{8}T\d{6}Z)-([0-9a-f]{12})\.tar\.gz$")
HEX_40 = re.compile(r"^[0-9a-f]{40}$")
HEX_64 = re.compile(r"^[0-9a-f]{64}$")


def _reject_duplicate_keys(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate JSON field")
        result[key] = value
    return result


def validate_record(record: Any, *, now_unix: int | None = None) -> dict[str, Any]:
    if not isinstance(record, dict) or set(record) != FIELDS:
        raise ValueError("report fields do not match the expected schema")
    integers = ("schema_version", "last_success_unix", "verified_generation_count", "retained_payload_bytes")
    if any(type(record[name]) is not int for name in integers):
        raise ValueError("report integer fields have invalid types")
    if record["schema_version"] != 1:
        raise ValueError("unsupported report schema")
    now = int(dt.datetime.now(dt.timezone.utc).timestamp()) if now_unix is None else now_unix
    stamp = record["last_success_unix"]
    if stamp <= 0 or stamp > now + 300:
        raise ValueError("report timestamp is invalid")
    archive = record["latest_archive"]
    match = ARCHIVE.fullmatch(archive) if isinstance(archive, str) and len(archive) <= 128 else None
    if not match:
        raise ValueError("archive name is invalid")
    try:
        dt.datetime.strptime(match.group(1), "%Y%m%dT%H%M%SZ")
    except ValueError as error:
        raise ValueError("archive timestamp is invalid") from error
    if not isinstance(record["latest_archive_sha256"], str) or not HEX_64.fullmatch(record["latest_archive_sha256"]):
        raise ValueError("archive digest is invalid")
    if not 1 <= record["verified_generation_count"] <= MAX_GENERATIONS:
        raise ValueError("verified generation count is invalid")
    if not 0 < record["retained_payload_bytes"] <= MAX_PAYLOAD_BYTES:
        raise ValueError("retained payload size is invalid")
    if not isinstance(record["source_commit"], str) or not HEX_40.fullmatch(record["source_commit"]):
        raise ValueError("source revision is invalid")
    if type(record["latest_rehashed"]) is not bool or not record["latest_rehashed"]:
        raise ValueError("latest archive was not rehashed")
    if type(record["recovery_kit_available"]) is not bool or not record["recovery_kit_available"]:
        raise ValueError("source-pinned recovery kit is not available")
    return {name: record[name] for name in sorted(FIELDS)}


def record_encoded(encoded: str, root: Path, *, now_unix: int | None = None) -> dict[str, Any]:
    if not isinstance(encoded, str) or not encoded or len(encoded) > MAX_ENCODED_BYTES:
        raise ValueError("encoded report is empty or oversized")
    try:
        report = json.loads(base64.b64decode(encoded, validate=True), object_pairs_hook=_reject_duplicate_keys)
    except (binascii.Error, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ValueError("encoded report is invalid") from error
    report = validate_record(report, now_unix=now_unix)
    root = Path(root)
    metadata = os.lstat(root)
    if not stat.S_ISDIR(metadata.st_mode) or stat.S_ISLNK(metadata.st_mode):
        raise ValueError("state-only backup root is not a real directory")
    destination = root / "owner-usb-sync.json"
    temporary = root / f".owner-usb-sync.{secrets.token_hex(12)}.partial"
    data = (json.dumps(report, separators=(",", ":"), sort_keys=True) + "\n").encode()
    fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0), 0o600)
    try:
        with os.fdopen(fd, "wb") as stream:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, destination)
        if os.name == "posix":
            os.chmod(destination, 0o600, follow_symlinks=False)
            dir_fd = os.open(root, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0))
            try:
                os.fsync(dir_fd)
            finally:
                os.close(dir_fd)
    finally:
        try:
            temporary.unlink()
        except FileNotFoundError:
            pass
    return report


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base64", required=True, dest="encoded")
    parser.add_argument("--root", default="/var/backups/vapor-server/state-only")
    args = parser.parse_args()
    try:
        record_encoded(args.encoded, Path(args.root))
    except (OSError, ValueError) as error:
        print(f"owner state USB status rejected: {error}", file=sys.stderr)
        return 2
    print("owner state USB sync status recorded")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
