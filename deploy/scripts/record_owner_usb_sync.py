#!/usr/bin/env python3
"""Record a bounded owner-reported USB recovery sync summary."""

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
MAX_ARCHIVES = 10_000
MAX_PAYLOAD_BYTES = 1 << 40
FILENAME = re.compile(
    r"^vapor-platform-recovery-(\d{8}T\d{6}Z)-([0-9a-f]{12})\.tar\.gz$"
)
HEX_40 = re.compile(r"^[0-9a-f]{40}$")
HEX_64 = re.compile(r"^[0-9a-f]{64}$")
FIELDS = {
    "schema_version",
    "last_success_unix",
    "latest_archive",
    "latest_archive_sha256",
    "verified_archive_count",
    "unverified_archive_count",
    "retained_archive_payload_bytes",
    "latest_rehashed",
    "recovery_kit_available",
    "recovery_kit_preflight_validation",
    "recovery_kit_source_commit",
}


def _reject_duplicate_keys(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate JSON field")
        result[key] = value
    return result


def encode_record(record: dict[str, Any]) -> str:
    return base64.b64encode(json.dumps(record, separators=(",", ":")).encode()).decode()


def validate_record(record: Any, *, now_unix: int | None = None) -> dict[str, Any]:
    if not isinstance(record, dict) or set(record) != FIELDS:
        raise ValueError("report fields do not match the expected schema")
    integer_fields = (
        "schema_version",
        "last_success_unix",
        "verified_archive_count",
        "unverified_archive_count",
        "retained_archive_payload_bytes",
    )
    if any(type(record[name]) is not int for name in integer_fields):
        raise ValueError("report integer fields have invalid types")
    if record["schema_version"] != 1:
        raise ValueError("unsupported report schema")
    stamp = record["last_success_unix"]
    now = int(dt.datetime.now(dt.timezone.utc).timestamp()) if now_unix is None else now_unix
    if stamp <= 0 or stamp > now + 300:
        raise ValueError("report timestamp is invalid")
    archive = record["latest_archive"]
    if not isinstance(archive, str) or len(archive) > 128:
        raise ValueError("archive name is invalid")
    match = FILENAME.fullmatch(archive)
    if not match:
        raise ValueError("archive name is invalid")
    try:
        dt.datetime.strptime(match.group(1), "%Y%m%dT%H%M%SZ")
    except ValueError as error:
        raise ValueError("archive timestamp is invalid") from error
    if not isinstance(record["latest_archive_sha256"], str) or not HEX_64.fullmatch(
        record["latest_archive_sha256"]
    ):
        raise ValueError("archive digest is invalid")
    verified = record["verified_archive_count"]
    unverified = record["unverified_archive_count"]
    payload_bytes = record["retained_archive_payload_bytes"]
    if not 1 <= verified <= MAX_ARCHIVES or not 0 <= unverified <= MAX_ARCHIVES:
        raise ValueError("archive verification summary is inconsistent")
    if not 0 < payload_bytes <= MAX_PAYLOAD_BYTES:
        raise ValueError("archive payload size is invalid")
    for name in ("latest_rehashed", "recovery_kit_available", "recovery_kit_preflight_validation"):
        if type(record[name]) is not bool:
            raise ValueError("report boolean fields have invalid types")
    if not record["latest_rehashed"]:
        raise ValueError("latest archive was not rehashed")
    kit_commit = record["recovery_kit_source_commit"]
    if record["recovery_kit_available"]:
        if not record["recovery_kit_preflight_validation"]:
            raise ValueError("recovery kit preflight was not validated")
        if not isinstance(kit_commit, str) or not HEX_40.fullmatch(kit_commit):
            raise ValueError("recovery kit source commit is invalid")
    elif record["recovery_kit_preflight_validation"] or kit_commit is not None:
        raise ValueError("unavailable recovery kit has inconsistent metadata")
    # Return only the explicit schema fields; never retain arbitrary client data.
    return {name: record[name] for name in sorted(FIELDS)}


def record_encoded(encoded: str, root: Path, *, now_unix: int | None = None) -> dict[str, Any]:
    if not isinstance(encoded, str) or not encoded or len(encoded) > MAX_ENCODED_BYTES:
        raise ValueError("encoded report is empty or oversized")
    try:
        raw = base64.b64decode(encoded, validate=True)
        report = json.loads(raw, object_pairs_hook=_reject_duplicate_keys)
    except (binascii.Error, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ValueError("encoded report is invalid") from error
    report = validate_record(report, now_unix=now_unix)
    root = Path(root)
    metadata = os.lstat(root)
    if not stat.S_ISDIR(metadata.st_mode) or stat.S_ISLNK(metadata.st_mode):
        raise ValueError("backup root is not a real directory")
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
    parser.add_argument("--root", default="/var/backups/vapor-server")
    args = parser.parse_args()
    try:
        record_encoded(args.encoded, Path(args.root))
    except (OSError, ValueError) as error:
        print(f"owner USB status rejected: {error}", file=sys.stderr)
        return 2
    print("owner USB sync status recorded")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
