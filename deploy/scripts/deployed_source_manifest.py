#!/usr/bin/env python3
"""Validate and atomically update the non-secret deployed-source inventory."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import pathlib
import re
import subprocess
import stat
import tempfile

from service_deploy_map import DEPLOY_TARGETS

try:
    import grp
except ImportError:  # Windows unit tests do not have POSIX group ownership.
    grp = None


TARGETS = set(DEPLOY_TARGETS)
COMMIT = re.compile(r"[0-9a-f]{40}\Z")


class ManifestError(ValueError):
    pass


def validate_manifest_data(data: object) -> dict:
    if not isinstance(data, dict) or set(data) != {"schema_version", "targets"}:
        raise ManifestError("deployed-source manifest must contain only schema_version and targets")
    if data.get("schema_version") != 1:
        raise ManifestError("deployed-source manifest has an unsupported schema")
    targets = data.get("targets")
    if not isinstance(targets, dict) or len(targets) > len(TARGETS):
        raise ManifestError("deployed-source target map is invalid")
    for target, entry in targets.items():
        if target not in TARGETS:
            raise ManifestError("deployed-source manifest contains an unknown target")
        if not isinstance(entry, dict) or set(entry) != {
            "composition_commit", "source_commit", "deployed_at_utc"
        }:
            raise ManifestError("deployed-source entry has an invalid shape")
        if not isinstance(entry["composition_commit"], str) or not COMMIT.fullmatch(entry["composition_commit"]):
            raise ManifestError("deployed-source composition revision is invalid")
        if not isinstance(entry["source_commit"], str) or not COMMIT.fullmatch(entry["source_commit"]):
            raise ManifestError("deployed-source target revision is invalid")
        timestamp = entry["deployed_at_utc"]
        if not isinstance(timestamp, str) or not timestamp.endswith("Z"):
            raise ManifestError("deployed-source timestamp is invalid")
        try:
            dt.datetime.fromisoformat(timestamp[:-1] + "+00:00")
        except ValueError as error:
            raise ManifestError("deployed-source timestamp is invalid") from error
    return data


def validate_manifest_text(raw: str) -> dict:
    try:
        data = json.loads(raw)
    except (json.JSONDecodeError, UnicodeError) as error:
        raise ManifestError("deployed-source manifest is not valid JSON") from error
    return validate_manifest_data(data)


def _safe_regular_file(path: pathlib.Path) -> bool:
    try:
        mode = path.lstat().st_mode
    except FileNotFoundError:
        return False
    if stat.S_ISLNK(mode) or not stat.S_ISREG(mode):
        raise ManifestError("deployed-source manifest path is not a regular file")
    return True


def _reject_symlink_components(path: pathlib.Path) -> pathlib.Path:
    if not path.is_absolute():
        raise ManifestError("deployed-source manifest path must be absolute")
    if ".." in path.parts:
        raise ManifestError("deployed-source manifest path must not contain parent traversal")
    normalized = pathlib.Path(os.path.abspath(path))
    current = pathlib.Path(normalized.anchor)
    for part in normalized.parts[1:]:
        current = current / part
        try:
            mode = current.lstat().st_mode
        except FileNotFoundError:
            continue
        if stat.S_ISLNK(mode):
            raise ManifestError("deployed-source manifest path contains a symlink component")
    return normalized


def update_target(path: pathlib.Path, target: str, composition_commit: str, source_commit: str) -> None:
    if target not in TARGETS:
        raise ManifestError("target is not allowlisted")
    if not COMMIT.fullmatch(composition_commit) or not COMMIT.fullmatch(source_commit):
        raise ManifestError("deployment revisions must be full lowercase Git commits")
    path = _reject_symlink_components(path)
    parent = path.parent
    if parent.exists() and (parent.is_symlink() or not parent.is_dir()):
        raise ManifestError("deployed-source manifest directory is unsafe")
    parent.mkdir(parents=True, exist_ok=True, mode=0o750)
    if _safe_regular_file(path):
        data = validate_manifest_text(path.read_text(encoding="utf-8"))
    else:
        data = {"schema_version": 1, "targets": {}}
    now = dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")
    data["targets"][target] = {
        "composition_commit": composition_commit,
        "source_commit": source_commit,
        "deployed_at_utc": now,
    }
    validate_manifest_data(data)
    _write_manifest(path, data)


def update_all_from_root(path: pathlib.Path, root: pathlib.Path) -> None:
    root = _reject_symlink_components(root)
    def git(*args: str) -> str:
        return subprocess.check_output(["git", "-C", str(root), *args], text=True).strip()

    composition_commit = git("rev-parse", "HEAD")
    if not COMMIT.fullmatch(composition_commit):
        raise ManifestError("Platform root revision is invalid")
    entries = {}
    now = dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")
    for target, mapping in DEPLOY_TARGETS.items():
        package_path = mapping["package_path"]
        package = root / package_path
        if package.is_symlink() or not package.is_dir():
            raise ManifestError(f"deployed source is missing for {target}")
        if mapping["kind"] == "root-service":
            source_commit = composition_commit
            entries[target] = {
                "composition_commit": composition_commit,
                "source_commit": source_commit,
                "deployed_at_utc": now,
            }
            continue
        source_commit = subprocess.check_output(
            ["git", "-C", str(package), "rev-parse", "HEAD"], text=True
        ).strip()
        if not COMMIT.fullmatch(source_commit):
            raise ManifestError(f"deployed source revision is invalid for {target}")
        fields = git("ls-tree", composition_commit, "--", package_path).split()
        if len(fields) != 4 or fields[:2] != ["160000", "commit"] or fields[2] != source_commit:
            raise ManifestError(f"deployed source does not match the Platform pin for {target}")
        entries[target] = {
            "composition_commit": composition_commit,
            "source_commit": source_commit,
            "deployed_at_utc": now,
        }
    data = {"schema_version": 1, "targets": entries}
    validate_manifest_data(data)
    _write_manifest(path, data)


def _write_manifest(path: pathlib.Path, data: dict) -> None:
    path = _reject_symlink_components(path)
    parent = path.parent
    if parent.exists() and (parent.is_symlink() or not parent.is_dir()):
        raise ManifestError("deployed-source manifest directory is unsafe")
    parent.mkdir(parents=True, exist_ok=True, mode=0o750)
    if _safe_regular_file(path):
        validate_manifest_text(path.read_text(encoding="utf-8"))
    temporary_name = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w", encoding="utf-8", newline="\n", dir=parent,
            prefix=f".{path.name}.", suffix=".tmp", delete=False,
        ) as output:
            temporary_name = output.name
            json.dump(data, output, separators=(",", ":"), sort_keys=True)
            output.write("\n")
            output.flush()
            os.fsync(output.fileno())
        os.chmod(temporary_name, 0o640)
        group_name = os.environ.get("VAPOR_GROUP", "vapor")
        if grp is not None and hasattr(os, "chown"):
            try:
                os.chown(temporary_name, 0, grp.getgrnam(group_name).gr_gid)
            except (KeyError, PermissionError):
                if getattr(os, "geteuid", lambda: 1)() == 0:
                    raise ManifestError("could not assign the deployed-source manifest group")
        os.replace(temporary_name, path)
        if os.name == "posix":
            directory_fd = os.open(parent, os.O_RDONLY)
            try:
                os.fsync(directory_fd)
            finally:
                os.close(directory_fd)
    finally:
        if temporary_name and os.path.exists(temporary_name):
            os.unlink(temporary_name)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--file", required=True, type=pathlib.Path)
    parser.add_argument("--root", type=pathlib.Path)
    parser.add_argument("--target")
    parser.add_argument("--composition-commit")
    parser.add_argument("--source-commit")
    arguments = parser.parse_args()
    if arguments.root is not None:
        if any(value is not None for value in (arguments.target, arguments.composition_commit, arguments.source_commit)):
            raise ManifestError("--root cannot be combined with one-target fields")
        update_all_from_root(arguments.file, arguments.root)
    elif arguments.target and arguments.composition_commit and arguments.source_commit:
        update_target(arguments.file, arguments.target, arguments.composition_commit, arguments.source_commit)
    else:
        raise ManifestError("provide --root or the complete one-target argument set")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
