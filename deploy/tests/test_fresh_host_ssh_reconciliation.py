from __future__ import annotations

import pathlib
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from contextlib import closing


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
SCRIPT = REPO_ROOT / "deploy/scripts/reconcile-fresh-host-ssh.py"


class FreshHostSshReconciliationTests(unittest.TestCase):
    def test_fresh_host_restore_retires_grants_without_erasing_pending_requests(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            database = pathlib.Path(directory) / "platform.sqlite3"
            with closing(sqlite3.connect(database)) as connection:
                connection.executescript(
                    """
                    CREATE TABLE platform_ssh_access_requests (
                        id TEXT PRIMARY KEY, public_key TEXT NOT NULL, status TEXT NOT NULL,
                        version INTEGER NOT NULL, granted_capabilities_json TEXT NOT NULL,
                        granted_duration_seconds INTEGER,
                        expires_at_unix INTEGER, decided_by_profile_id TEXT,
                        decided_by_actor_kind TEXT NOT NULL, decided_by_actor_label TEXT,
                        decided_at_unix INTEGER, operation_detail TEXT
                    );
                    CREATE TABLE platform_ssh_access_events (
                        event_id INTEGER PRIMARY KEY AUTOINCREMENT, request_id TEXT NOT NULL,
                        from_status TEXT, to_status TEXT NOT NULL, actor_profile_id TEXT,
                        actor_kind TEXT NOT NULL, actor_label TEXT, at_unix INTEGER NOT NULL
                    );
                    CREATE TRIGGER platform_ssh_access_update_audit
                    AFTER UPDATE OF status ON platform_ssh_access_requests
                    WHEN OLD.status != NEW.status
                    BEGIN
                      INSERT INTO platform_ssh_access_events(
                        request_id, from_status, to_status, actor_profile_id,
                        actor_kind, actor_label, at_unix
                      ) VALUES (
                        NEW.id, OLD.status, NEW.status, NEW.decided_by_profile_id,
                        NEW.decided_by_actor_kind, NEW.decided_by_actor_label,
                        NEW.decided_at_unix
                      );
                    END;
                    INSERT INTO platform_ssh_access_requests VALUES
                      ('active', 'ssh-ed25519 active', 'active', 4, '["host_admin"]', 28800, 9999999999, 'p1', 'identity_profile', NULL, 10, NULL),
                      ('failed', 'ssh-ed25519 failed', 'failed', 7, '["vapor_deploy"]', 3600, 9999999999, NULL, 'local_operator', 'local-root', 20, 'failed cleanup'),
                      ('provisioning', 'ssh-ed25519 partial', 'provisioning', 2, '["host_admin"]', 28800, 9999999999, NULL, 'local_operator', 'local-root', 30, 'granting'),
                      ('pending', 'ssh-ed25519 pending', 'pending', 1, '[]', NULL, NULL, NULL, 'identity_profile', NULL, NULL, NULL);
                    """
                )
                connection.commit()

            result = subprocess.run(
                [sys.executable, str(SCRIPT), str(database)],
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("retired 3", result.stdout)

            with closing(sqlite3.connect(database)) as connection:
                rows = connection.execute(
                    "SELECT id,status,public_key,granted_capabilities_json,expires_at_unix,"
                    "decided_by_actor_kind,decided_by_actor_label FROM platform_ssh_access_requests ORDER BY id"
                ).fetchall()
                events = connection.execute(
                    "SELECT request_id,from_status,to_status,actor_kind,actor_label "
                    "FROM platform_ssh_access_events ORDER BY event_id"
                ).fetchall()

        self.assertEqual([row[1] for row in rows], ["revoked", "revoked", "pending", "revoked"])
        for row in (rows[0], rows[1], rows[3]):
            self.assertEqual(row[2], "")
            self.assertEqual(row[3], "[]")
            self.assertIsNone(row[4])
            self.assertEqual(row[5], "system_recovery")
            self.assertEqual(row[6], "fresh-host-restore")
        self.assertEqual(len(events), 3)
        self.assertTrue(all(event[2] == "revoked" for event in events))
        self.assertTrue(all(event[3:] == ("system_recovery", "fresh-host-restore") for event in events))

    def test_refuses_to_retire_grants_when_audit_trigger_is_missing(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            database = pathlib.Path(directory) / "platform.sqlite3"
            with closing(sqlite3.connect(database)) as connection:
                connection.executescript(
                    """
                    CREATE TABLE platform_ssh_access_requests (
                        id TEXT PRIMARY KEY, public_key TEXT NOT NULL, status TEXT NOT NULL,
                        version INTEGER NOT NULL, granted_capabilities_json TEXT NOT NULL,
                        granted_duration_seconds INTEGER, expires_at_unix INTEGER,
                        decided_by_profile_id TEXT, decided_by_actor_kind TEXT NOT NULL,
                        decided_by_actor_label TEXT, decided_at_unix INTEGER, operation_detail TEXT
                    );
                    CREATE TABLE platform_ssh_access_events (
                        event_id INTEGER PRIMARY KEY AUTOINCREMENT, request_id TEXT NOT NULL,
                        from_status TEXT, to_status TEXT NOT NULL, actor_profile_id TEXT,
                        actor_kind TEXT NOT NULL, actor_label TEXT, at_unix INTEGER NOT NULL
                    );
                    INSERT INTO platform_ssh_access_requests VALUES
                      ('active', 'ssh-ed25519 active', 'active', 4, '["host_admin"]', 28800, 9999999999, 'p1', 'identity_profile', NULL, 10, NULL);
                    """
                )
                connection.commit()

            result = subprocess.run(
                [sys.executable, str(SCRIPT), str(database)],
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(result.returncode, 1)
            self.assertIn("audit trigger is missing or outdated", result.stderr)

            with closing(sqlite3.connect(database)) as connection:
                row = connection.execute(
                    "SELECT status,version,public_key FROM platform_ssh_access_requests WHERE id='active'"
                ).fetchone()
            self.assertEqual(row, ("active", 4, "ssh-ed25519 active"))


if __name__ == "__main__":
    unittest.main()
