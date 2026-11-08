#!/usr/bin/env python3
"""Run SQLite quick_check on the required state-only service databases."""

from __future__ import annotations

import pathlib
import sqlite3
import sys


DATABASES = (
    "identity/identity.sqlite3",
    "platform/platform.sqlite3",
    "registry/registry.sqlite3",
)


def validate(root: pathlib.Path) -> None:
    for relative in DATABASES:
        path = root / relative
        if path.is_symlink() or not path.is_file():
            raise ValueError("required database is missing or unsafe")
        connection: sqlite3.Connection | None = None
        try:
            connection = sqlite3.connect(f"file:{path.as_posix()}?mode=ro", uri=True, timeout=5)
            rows = connection.execute("PRAGMA quick_check").fetchall()
        except sqlite3.Error as error:
            raise ValueError("required database failed SQLite quick_check") from error
        finally:
            if connection is not None:
                connection.close()
        if rows != [("ok",)]:
            raise ValueError("required database failed SQLite quick_check")


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print("usage: validate-state-databases.py STATE_ROOT", file=sys.stderr)
        return 2
    try:
        validate(pathlib.Path(argv[1]))
    except (OSError, ValueError) as error:
        print(f"state-database-check: {error}", file=sys.stderr)
        return 1
    print("state-database-check: all required SQLite databases are consistent")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
