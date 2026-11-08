#!/usr/bin/env python3
"""Manage the secret-free, archive-pinned state-only recovery marker."""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import re
import stat
import sys
import tempfile


MARKER_NAME = "state-only.json"
SHA256 = re.compile(r"[0-9a-f]{64}\Z")
COMMIT = re.compile(r"[0-9a-f]{40}\Z")
PHASES = {"building", "awaiting_provider_checks"}
EXPECTED_FIELDS = {"schema_version", "bundle_sha256", "root_commit", "phase"}
CANONICAL_ROOT = pathlib.Path("/var/lib/vapor-state-recovery")


class MarkerError(RuntimeError):
    pass


def _assert_secure_owner(path: pathlib.Path, info: os.stat_result, expected_mode: int) -> None:
    if os.name != "posix":
        return
    if stat.S_IMODE(info.st_mode) != expected_mode:
        raise MarkerError("recovery marker permissions are invalid")
    if path == CANONICAL_ROOT or path == CANONICAL_ROOT / MARKER_NAME:
        if not hasattr(os, "getuid") or info.st_uid != 0:
            raise MarkerError("recovery marker ownership is invalid")


def _validate_root(root: pathlib.Path, *, allow_empty: bool = False) -> pathlib.Path:
    root = pathlib.Path(os.path.abspath(root))
    if root.is_symlink():
        raise MarkerError("recovery marker directory must not be a symlink")
    if not root.exists():
        if not allow_empty:
            raise MarkerError("recovery marker directory is missing")
        return root
    info = root.lstat()
    if not stat.S_ISDIR(info.st_mode):
        raise MarkerError("recovery marker path is not a directory")
    _assert_secure_owner(root, info, 0o700)
    entries = {entry.name for entry in root.iterdir()}
    allowed = {MARKER_NAME} if MARKER_NAME in entries else set()
    if entries != allowed:
        raise MarkerError("recovery marker directory contains unexpected entries")
    return root


def _read(root: pathlib.Path, digest: str, revision: str) -> dict[str, object]:
    root = _validate_root(root)
    path = root / MARKER_NAME
    if path.is_symlink():
        raise MarkerError("recovery marker must not be a symlink")
    try:
        info = path.lstat()
    except FileNotFoundError as error:
        raise MarkerError("recovery marker is missing") from error
    if not stat.S_ISREG(info.st_mode):
        raise MarkerError("recovery marker is not a regular file")
    _assert_secure_owner(path, info, 0o600)
    if info.st_size > 1024:
        raise MarkerError("recovery marker is too large")
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise MarkerError("recovery marker is unreadable or invalid") from error
    if not isinstance(value, dict) or set(value) != EXPECTED_FIELDS:
        raise MarkerError("recovery marker fields are invalid")
    if (
        value["schema_version"] != 1
        or value["bundle_sha256"] != digest
        or value["root_commit"] != revision
        or value["phase"] not in PHASES
    ):
        raise MarkerError("recovery marker does not match this archive and source revision")
    return value


def _write(root: pathlib.Path, value: dict[str, object], *, create: bool) -> None:
    root = _validate_root(root, allow_empty=create)
    if create:
        if root.exists():
            if any(root.iterdir()):
                raise MarkerError("recovery marker path already contains data")
        else:
            root.mkdir(parents=True, mode=0o700)
        os.chmod(root, 0o700)
        if hasattr(os, "getuid") and root == CANONICAL_ROOT and root.stat().st_uid != 0:
            raise MarkerError("recovery marker directory must be owned by root")
    elif not (root / MARKER_NAME).is_file():
        raise MarkerError("recovery marker is missing")

    descriptor, temporary_name = tempfile.mkstemp(prefix=".state-only.", dir=root)
    temporary = pathlib.Path(temporary_name)
    try:
        os.fchmod(descriptor, 0o600)
        payload = json.dumps(value, separators=(",", ":"), sort_keys=True) + "\n"
        with os.fdopen(descriptor, "w", encoding="utf-8", newline="\n") as stream:
            stream.write(payload)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, root / MARKER_NAME)
        if hasattr(os, "O_DIRECTORY"):
            directory = os.open(root, os.O_RDONLY | os.O_DIRECTORY)
            try:
                os.fsync(directory)
            finally:
                os.close(directory)
    finally:
        try:
            temporary.unlink()
        except FileNotFoundError:
            pass


def _arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="action", required=True)
    for action in ("init", "read", "set-phase", "complete"):
        child = subparsers.add_parser(action)
        child.add_argument("--root", required=True, type=pathlib.Path)
        child.add_argument("--bundle-sha256", required=True)
        child.add_argument("--root-commit", required=True)
        if action != "read":
            child.add_argument("--phase", choices=sorted(PHASES)) if action == "set-phase" else None
    return parser.parse_args()


def main() -> int:
    args = _arguments()
    if not SHA256.fullmatch(args.bundle_sha256):
        raise MarkerError("bundle digest is invalid")
    if not COMMIT.fullmatch(args.root_commit):
        raise MarkerError("Platform source commit is invalid")

    if args.action == "init":
        _write(
            args.root,
            {
                "schema_version": 1,
                "bundle_sha256": args.bundle_sha256,
                "root_commit": args.root_commit,
                "phase": "building",
            },
            create=True,
        )
        print("building")
        return 0

    marker = _read(args.root, args.bundle_sha256, args.root_commit)
    if args.action == "read":
        print(json.dumps(marker, separators=(",", ":"), sort_keys=True))
        return 0
    if args.action == "set-phase":
        current = str(marker["phase"])
        requested = args.phase
        if requested == "building" and current != "building":
            raise MarkerError("recovery phase cannot move backwards")
        if requested == "awaiting_provider_checks" and current not in {
            "building",
            "awaiting_provider_checks",
        }:
            raise MarkerError("recovery phase transition is invalid")
        marker["phase"] = requested
        _write(args.root, marker, create=False)
        print(requested)
        return 0
    if args.action == "complete":
        if marker["phase"] != "awaiting_provider_checks":
            raise MarkerError("owner provider checks are not awaiting confirmation")
        marker_path = args.root / MARKER_NAME
        marker_path.unlink()
        args.root.rmdir()
        print("complete")
        return 0
    raise MarkerError("unsupported recovery marker action")


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except MarkerError as error:
        print(f"state-only-recovery-marker: {error}", file=sys.stderr)
        raise SystemExit(1)
