from __future__ import annotations

import contextlib
import importlib.util
import io
import pathlib
import sqlite3
import unittest
from unittest import mock


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
QUEUE_HELPER = REPO_ROOT / "deploy" / "scripts" / "deploy-request-queue.py"
SPEC = importlib.util.spec_from_file_location("deploy_request_queue", QUEUE_HELPER)
QUEUE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(QUEUE)


class DeployRequestQueueTests(unittest.TestCase):
    def test_expired_queued_request_is_reported_as_expired_before_claim(self) -> None:
        db = sqlite3.connect(":memory:", isolation_level=None)
        db.row_factory = sqlite3.Row
        db.execute(
            "CREATE TABLE platform_deploy_requests ("
            "id TEXT PRIMARY KEY, status TEXT NOT NULL, version INTEGER NOT NULL, "
            "expires_at_unix INTEGER NOT NULL, decided_at_unix INTEGER, "
            "dispatch_state TEXT NOT NULL, dispatch_detail TEXT)"
        )
        db.execute(
            "INSERT INTO platform_deploy_requests "
            "(id, status, version, expires_at_unix, dispatch_state) "
            "VALUES ('request-1', 'approved', 2, 99, 'queued')"
        )
        output = io.StringIO()

        with mock.patch.object(QUEUE.time, "time", return_value=100), contextlib.redirect_stdout(output):
            result = QUEUE.claim(db, "request-1")

        row = db.execute(
            "SELECT status, dispatch_state, dispatch_detail "
            "FROM platform_deploy_requests WHERE id='request-1'"
        ).fetchone()
        self.assertEqual(result, 0)
        self.assertEqual(output.getvalue().strip(), "null")
        self.assertEqual(row["status"], "expired")
        self.assertEqual(row["dispatch_state"], "expired")
        self.assertIn("expired before the worker claimed", row["dispatch_detail"])
        db.close()


if __name__ == "__main__":
    unittest.main()
