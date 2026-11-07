#!/usr/bin/env python3
"""Retire temporary SSH grants that are not transferred to a fresh host."""

from __future__ import annotations

import pathlib
import sqlite3
import sys
import time


REQUIRED_REQUEST_COLUMNS = {
    "id",
    "public_key",
    "granted_capabilities_json",
    "granted_duration_seconds",
    "status",
    "version",
    "expires_at_unix",
    "decided_by_profile_id",
    "decided_by_actor_kind",
    "decided_by_actor_label",
    "decided_at_unix",
    "operation_detail",
}
REQUIRED_EVENT_COLUMNS = {
    "request_id",
    "from_status",
    "to_status",
    "actor_profile_id",
    "actor_kind",
    "actor_label",
    "at_unix",
}


def retire_untransferred_grants(database_path: pathlib.Path) -> int:
    if database_path.is_symlink() or not database_path.is_file():
        raise ValueError("Platform database is missing or is not a regular file")

    connection = sqlite3.connect(database_path, timeout=10)
    try:
        connection.execute("PRAGMA foreign_keys = ON")
        columns = {
            row[1]
            for row in connection.execute("PRAGMA table_info(platform_ssh_access_requests)")
        }
        if not REQUIRED_REQUEST_COLUMNS.issubset(columns):
            raise ValueError("Platform SSH access schema is incomplete; refusing recovery")
        event_columns = {
            row[1]
            for row in connection.execute("PRAGMA table_info(platform_ssh_access_events)")
        }
        if not REQUIRED_EVENT_COLUMNS.issubset(event_columns):
            raise ValueError("Platform SSH audit table is missing; refusing recovery")
        trigger = connection.execute(
            "SELECT sql FROM sqlite_master "
            "WHERE type='trigger' AND name='platform_ssh_access_update_audit'"
        ).fetchone()
        trigger_sql = trigger[0] if trigger else None
        if not isinstance(trigger_sql, str) or not all(
            token in trigger_sql.lower()
            for token in (
                "after update of status",
                "platform_ssh_access_events",
                "actor_kind",
                "actor_label",
            )
        ):
            raise ValueError("Platform SSH audit trigger is missing or outdated; refusing recovery")

        now = int(time.time())
        connection.execute("BEGIN IMMEDIATE")
        result = connection.execute(
            "UPDATE platform_ssh_access_requests "
            "SET status='revoked', version=version+1, public_key='', "
            "granted_capabilities_json='[]', granted_duration_seconds=NULL, "
            "expires_at_unix=NULL, decided_by_profile_id=NULL, "
            "decided_by_actor_kind='system_recovery', "
            "decided_by_actor_label='fresh-host-restore', decided_at_unix=?, "
            "operation_detail=NULL "
            "WHERE status IN ('active','failed','provisioning')",
            (now,),
        )
        count = result.rowcount
        connection.commit()
        return count
    except Exception:
        connection.rollback()
        raise
    finally:
        connection.close()


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: reconcile-fresh-host-ssh.py /path/to/platform.sqlite3", file=sys.stderr)
        return 2
    try:
        retired = retire_untransferred_grants(pathlib.Path(sys.argv[1]))
    except (OSError, sqlite3.Error, ValueError) as error:
        print(f"fresh-recovery: SSH grant reconciliation failed: {error}", file=sys.stderr)
        return 1
    print(f"fresh-recovery: retired {retired} temporary SSH grant record(s) not transferred to this host")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
