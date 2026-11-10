#!/usr/bin/env python3
"""Append bounded-metadata deploy/restore outcomes to Platform's own database."""

from __future__ import annotations

import argparse
from contextlib import closing
import datetime as dt
import os
import pathlib
import re
import sqlite3
import sys
import uuid

TIMESTAMP = re.compile(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$")
REVISION = re.compile(r"^[0-9A-Fa-f]{7,40}$")
SCHEMA = """
CREATE TABLE IF NOT EXISTS platform_operation_history (
    operation_id TEXT PRIMARY KEY,
    operation TEXT NOT NULL CHECK(operation IN ('deploy','restore')),
    result TEXT NOT NULL CHECK(result IN ('success','failed')),
    started_at_unix INTEGER NOT NULL,
    finished_at_unix INTEGER NOT NULL CHECK(finished_at_unix >= started_at_unix),
    started_at_utc TEXT NOT NULL,
    finished_at_utc TEXT NOT NULL,
    revision TEXT NOT NULL,
    server_mode TEXT NOT NULL CHECK(server_mode = 'services')
)
"""


def _timestamp(value: str) -> dt.datetime:
    if not TIMESTAMP.fullmatch(value):
        raise ValueError("timestamp must be canonical UTC")
    try:
        return dt.datetime.strptime(value, "%Y-%m-%dT%H:%M:%SZ").replace(
            tzinfo=dt.timezone.utc
        )
    except ValueError as error:
        raise ValueError("timestamp is invalid") from error


def record_operation(
    database: pathlib.Path,
    operation: str,
    result: str,
    started_at_utc: str,
    finished_at_utc: str,
    revision: str,
    server_mode: str,
) -> str:
    if os.name == "posix" and hasattr(os, "geteuid") and os.geteuid() == 0:
        raise PermissionError("operation history must be written as the Platform service account")
    if operation not in {"deploy", "restore"} or result not in {"success", "failed"}:
        raise ValueError("operation and result must use the Platform allowlist")
    if server_mode != "services":
        raise ValueError("server mode must use the Platform allowlist")
    if revision != "unknown" and not REVISION.fullmatch(revision):
        raise ValueError("revision is invalid")
    started = _timestamp(started_at_utc)
    finished = _timestamp(finished_at_utc)
    if finished < started:
        raise ValueError("finish time precedes start time")

    database = pathlib.Path(database)
    if database.parent.is_symlink() or database.is_symlink():
        raise ValueError("refusing a symlinked operation-history path")
    database.parent.mkdir(mode=0o750, parents=True, exist_ok=True)
    operation_id = str(uuid.uuid4())
    previous_umask = os.umask(0o027)
    try:
        with closing(sqlite3.connect(database, timeout=5)) as connection:
            with connection:
                connection.execute("PRAGMA busy_timeout=5000")
                connection.execute("PRAGMA journal_mode=WAL")
                connection.execute(SCHEMA)
                connection.execute(
                    "CREATE INDEX IF NOT EXISTS idx_platform_operation_history_finished "
                    "ON platform_operation_history(finished_at_unix DESC, operation_id DESC)"
                )
                connection.execute(
                    "INSERT INTO platform_operation_history "
                    "(operation_id, operation, result, started_at_unix, finished_at_unix, "
                    "started_at_utc, finished_at_utc, revision, server_mode) "
                    "VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    (
                        operation_id,
                        operation,
                        result,
                        int(started.timestamp()),
                        int(finished.timestamp()),
                        started_at_utc,
                        finished_at_utc,
                        revision,
                        server_mode,
                    ),
                )
    finally:
        os.umask(previous_umask)

    if database.exists():
        database.chmod(0o640)
    return operation_id


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("database", type=pathlib.Path)
    parser.add_argument("operation", choices=("deploy", "restore"))
    parser.add_argument("result", choices=("success", "failed"))
    parser.add_argument("started_at_utc")
    parser.add_argument("finished_at_utc")
    parser.add_argument("revision")
    parser.add_argument("server_mode", choices=("services",))
    args = parser.parse_args(argv)
    try:
        record_operation(
            args.database,
            args.operation,
            args.result,
            args.started_at_utc,
            args.finished_at_utc,
            args.revision,
            args.server_mode,
        )
    except (OSError, sqlite3.Error, ValueError) as error:
        print(f"operation-history: could not append record: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
