#!/usr/bin/env python3
"""Take coherent online SQLite backups of the required Vapor state databases."""

from __future__ import annotations

import pathlib
import sqlite3
import sys
from urllib.parse import quote


DATABASES = (
    "identity/identity.sqlite3",
    "platform/platform.sqlite3",
    "registry/registry.sqlite3",
)


def open_readonly(path: pathlib.Path) -> sqlite3.Connection:
    uri = f"file:{quote(path.resolve().as_posix(), safe='/:')}?mode=ro"
    return sqlite3.connect(uri, uri=True, timeout=30)


def snapshot(source_root: pathlib.Path, destination_root: pathlib.Path) -> None:
    if source_root.is_symlink() or not source_root.is_dir():
        raise ValueError("source state root is missing or unsafe")
    if destination_root.is_symlink():
        raise ValueError("destination state root is missing or unsafe")
    if destination_root.exists() and (not destination_root.is_dir() or any(destination_root.iterdir())):
        raise ValueError("destination state root must be empty")

    for relative in DATABASES:
        source_path = source_root / relative
        destination_path = destination_root / relative
        if source_path.parent.is_symlink() or source_path.is_symlink() or not source_path.is_file():
            raise ValueError("required source database is missing or unsafe")
        destination_path.parent.mkdir(parents=True, exist_ok=True)
        source: sqlite3.Connection | None = None
        destination: sqlite3.Connection | None = None
        try:
            source = open_readonly(source_path)
            destination = sqlite3.connect(destination_path, timeout=30)
            source.backup(destination, pages=256, sleep=0.05)
            destination.execute("PRAGMA journal_mode=DELETE")
            destination.commit()
        except sqlite3.Error as error:
            raise ValueError("could not create a consistent online database snapshot") from error
        finally:
            if destination is not None:
                destination.close()
            if source is not None:
                source.close()
        destination_path.chmod(0o600)


def main(argv: list[str]) -> int:
    if len(argv) != 3:
        print("usage: snapshot-state-databases.py SOURCE_STATE_ROOT DESTINATION_STATE_ROOT", file=sys.stderr)
        return 2
    try:
        snapshot(pathlib.Path(argv[1]), pathlib.Path(argv[2]))
    except (OSError, ValueError) as error:
        print(f"state-database-snapshot: {error}", file=sys.stderr)
        return 1
    print("state-database-snapshot: all required databases captured consistently")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
