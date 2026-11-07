#!/usr/bin/env python3
"""Plan safe backup pruning while protecting the current and incoming generation."""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import re
import sys


ARCHIVE_NAME = re.compile(r"vapor-platform-recovery-\d{8}T\d{6}Z-[0-9a-f]{12}\.tar\.gz\Z")
SHA256 = re.compile(r"[0-9a-f]{64}\Z")


def _record(root: pathlib.Path, name: object) -> dict[str, object] | None:
    if not isinstance(name, str) or not ARCHIVE_NAME.fullmatch(name):
        return None
    archive = root / name
    sidecar = root / f"{name}.sha256"
    try:
        if archive.is_symlink() or not archive.is_file() or archive.stat().st_size <= 0:
            return None
        if sidecar.is_symlink() or not sidecar.is_file():
            return None
        fields = sidecar.read_text(encoding="ascii").strip().split(maxsplit=1)
        if len(fields) != 2 or not SHA256.fullmatch(fields[0]) or fields[1] != name:
            return None
        digest = hashlib.sha256()
        with archive.open("rb") as contents:
            for chunk in iter(lambda: contents.read(1024 * 1024), b""):
                digest.update(chunk)
        if digest.hexdigest() != fields[0]:
            return None
        return {"archive": name, "sha256": fields[0], "size_bytes": archive.stat().st_size}
    except (OSError, UnicodeError):
        return None


def _trusted_current(root: pathlib.Path, records: dict[str, dict[str, object]]) -> str | None:
    latest_path = root / "latest.json"
    index_path = root / "index.json"
    if not (latest_path.exists() or latest_path.is_symlink()) and not (index_path.exists() or index_path.is_symlink()):
        return None
    if latest_path.is_symlink() or index_path.is_symlink():
        return None
    if not latest_path.is_file() or not index_path.is_file():
        return None
    try:
        latest = json.loads(latest_path.read_text(encoding="utf-8"))
        index = json.loads(index_path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError):
        return None
    if not isinstance(latest, dict) or latest.get("schema_version") != 1:
        return None
    if not isinstance(index, dict) or index.get("schema_version") != 1 or not isinstance(index.get("archives"), list):
        return None
    name = latest.get("archive")
    record = records.get(name) if isinstance(name, str) else None
    if record is None or latest.get("sha256") != record["sha256"] or latest.get("size_bytes") != record["size_bytes"]:
        return None
    if not any(
        isinstance(item, dict)
        and item.get("archive") == name
        and item.get("sha256") == record["sha256"]
        and item.get("size_bytes") == record["size_bytes"]
        for item in index["archives"]
    ):
        return None
    return name


def plan_prune_names(root: pathlib.Path, new_name: str, keep: int) -> tuple[str, ...]:
    if keep < 2:
        raise ValueError("retention must keep at least two generations")
    if not ARCHIVE_NAME.fullmatch(new_name):
        raise ValueError("incoming archive name is invalid")
    records = {
        name: record
        for path in root.iterdir()
        if path.name.endswith(".tar.gz")
        if (record := _record(root, name := path.name)) is not None
    }
    new_record = _record(root, new_name)
    if new_record is None:
        raise ValueError("incoming archive or checksum is missing or invalid")
    records[new_name] = new_record
    current = _trusted_current(root, records)
    metadata_present = any(
        path.exists() or path.is_symlink()
        for path in (root / "latest.json", root / "index.json")
    )
    if metadata_present:
        if current is None:
            return ()
    elif any(name != new_name for name in records):
        return ()
    protected = {new_name}
    if current:
        protected.add(current)
    ordered = sorted(records)
    excess = max(0, len(ordered) - keep)
    removals: list[str] = []
    for name in ordered:
        if excess <= 0:
            break
        if name in protected:
            continue
        removals.append(name)
        excess -= 1
    return tuple(removals)


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=pathlib.Path, required=True)
    parser.add_argument("--new", required=True)
    parser.add_argument("--keep", type=int, required=True)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    arguments = parser.parse_args(argv)
    try:
        removals = plan_prune_names(arguments.root, arguments.new, arguments.keep)
    except (OSError, ValueError) as error:
        print(f"recovery-retention: {error}", file=sys.stderr)
        return 1
    arguments.output.write_text("".join(f"{name}\n" for name in removals), encoding="ascii")
    if not removals and ((arguments.root / "latest.json").exists() or (arguments.root / "index.json").exists()):
        print("recovery-retention: existing latest/index metadata is not consistent; pruning skipped", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
