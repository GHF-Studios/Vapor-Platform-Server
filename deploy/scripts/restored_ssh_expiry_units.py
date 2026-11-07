#!/usr/bin/env python3
"""List validated expiry timer units for active grants in restored Platform state."""

from __future__ import annotations

import pathlib
import sqlite3
import sys
import uuid


def restored_active_timer_units(database: pathlib.Path, unit_directory: pathlib.Path) -> list[str]:
    if database.is_symlink() or not database.is_file():
        raise ValueError("Platform database is missing or is not a regular file")
    if unit_directory.is_symlink() or not unit_directory.is_dir():
        raise ValueError("systemd unit directory is missing or is not a regular directory")
    uri = database.resolve().as_uri() + "?mode=ro"
    with sqlite3.connect(uri, uri=True, timeout=5) as connection:
        rows = connection.execute(
            "SELECT id FROM platform_ssh_access_requests WHERE status='active' ORDER BY id"
        ).fetchall()

    units: list[str] = []
    for (request_id,) in rows:
        try:
            canonical_id = str(uuid.UUID(request_id))
        except (ValueError, TypeError, AttributeError) as error:
            raise ValueError("active SSH request has an invalid identifier") from error
        if canonical_id != request_id:
            raise ValueError("active SSH request identifier is not canonical")
        for suffix in ("timer", "service"):
            path = unit_directory / f"vapor-ssh-expire-{canonical_id}.{suffix}"
            if path.is_symlink() or not path.is_file():
                raise ValueError("active SSH request is missing its managed expiry unit")
        units.append(f"vapor-ssh-expire-{canonical_id}.timer")
    return units


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: restored_ssh_expiry_units.py PLATFORM_DB SYSTEMD_UNIT_DIR", file=sys.stderr)
        return 2
    try:
        units = restored_active_timer_units(pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]))
    except (OSError, sqlite3.Error, ValueError) as error:
        print(f"recovery-restore: SSH expiry unit validation failed: {error}", file=sys.stderr)
        return 1
    print("\n".join(units))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
