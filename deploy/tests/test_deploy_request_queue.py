from __future__ import annotations

import contextlib
import hashlib
import importlib.util
import io
import json
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
    def _database_with_request(self, request, status="approved", version=2):
        db = sqlite3.connect(":memory:", isolation_level=None)
        db.row_factory = sqlite3.Row
        db.execute(
            "CREATE TABLE platform_deploy_requests ("
            "id TEXT PRIMARY KEY, status TEXT NOT NULL, version INTEGER NOT NULL, "
            "expires_at_unix INTEGER NOT NULL, decided_at_unix INTEGER, "
            "dispatch_state TEXT NOT NULL, dispatch_detail TEXT, result_detail TEXT, "
            "body_hash TEXT NOT NULL, request_json TEXT NOT NULL, repository TEXT NOT NULL, "
            "branch TEXT NOT NULL, commit_hash TEXT NOT NULL, tree_hash TEXT NOT NULL, "
            "workflow_url TEXT NOT NULL, summary TEXT NOT NULL, submodules_json TEXT NOT NULL)"
        )
        raw = json.dumps(request, separators=(",", ":"))
        db.execute(
            "INSERT INTO platform_deploy_requests "
            "(id,status,version,expires_at_unix,decided_at_unix,dispatch_state,dispatch_detail,result_detail,"
            "body_hash,request_json,repository,branch,commit_hash,tree_hash,workflow_url,summary,submodules_json) "
            "VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            (
                "req", status, version, 9999999999, None, "not_attempted", None, None,
                hashlib.sha256(raw.encode()).hexdigest(), raw,
                request["repository"], request["branch"], request["commit"], request["tree"],
                request["workflow_url"], request["summary"],
                json.dumps(request["submodules"], separators=(",", ":")),
            ),
        )
        return db

    def _request(self, server_mode=None):
        request = {
            "repository": "GHF-Studios/Vapor-Platform-Server",
            "branch": "main",
            "commit": "0" * 40,
            "tree": "1" * 40,
            "submodules": [],
            "workflow_url": "https://github.com/GHF-Studios/Vapor-Platform-Server/actions/runs/1",
            "summary": "CI passed",
        }
        if server_mode is not None:
            request["server_mode"] = server_mode
        return request

    def test_claim_preserves_explicit_deployment_target(self) -> None:
        db = self._database_with_request(self._request("services"))
        output = io.StringIO()

        with contextlib.redirect_stdout(output):
            result = QUEUE.claim(db, "req")

        self.assertEqual(result, 0)
        self.assertEqual(json.loads(output.getvalue())["request"]["server_mode"], "services")
        db.close()

    def test_verify_rejects_target_different_from_approved_request(self) -> None:
        request = self._request("services")
        db = self._database_with_request(request, status="executing", version=3)

        with self.assertRaisesRegex(ValueError, "topology"):
            QUEUE.verify_claim(db, "req", 3, request["commit"], "monolith")

        db.close()

    def test_approved_legacy_request_without_target_is_failed_closed(self) -> None:
        db = sqlite3.connect(":memory:", isolation_level=None)
        db.row_factory = sqlite3.Row
        db.execute(
            "CREATE TABLE platform_deploy_requests ("
            "id TEXT PRIMARY KEY, status TEXT NOT NULL, version INTEGER NOT NULL, "
            "expires_at_unix INTEGER NOT NULL, decided_at_unix INTEGER, "
            "dispatch_state TEXT NOT NULL, dispatch_detail TEXT, result_detail TEXT, "
            "body_hash TEXT NOT NULL, request_json TEXT NOT NULL, repository TEXT NOT NULL, "
            "branch TEXT NOT NULL, commit_hash TEXT NOT NULL, tree_hash TEXT NOT NULL, "
            "workflow_url TEXT NOT NULL, summary TEXT NOT NULL, submodules_json TEXT NOT NULL)"
        )
        request = self._request()
        request["summary"] = "old request without target"
        raw = json.dumps(request, separators=(",", ":"))
        db.execute(
            "INSERT INTO platform_deploy_requests "
            "(id,status,version,expires_at_unix,decided_at_unix,dispatch_state,dispatch_detail,result_detail,"
            "body_hash,request_json,repository,branch,commit_hash,tree_hash,workflow_url,summary,submodules_json) "
            "VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            (
                "legacy", "approved", 2, 9999999999, None, "queued", None, None,
                hashlib.sha256(raw.encode()).hexdigest(), raw, request["repository"],
                request["branch"], request["commit"], request["tree"],
                request["workflow_url"], request["summary"], "[]",
            ),
        )
        output = io.StringIO()

        with contextlib.redirect_stdout(output):
            result = QUEUE.claim(db, "legacy")

        row = db.execute(
            "SELECT status, dispatch_state, dispatch_detail, result_detail "
            "FROM platform_deploy_requests WHERE id='legacy'"
        ).fetchone()
        self.assertEqual(result, 0)
        self.assertEqual(output.getvalue().strip(), "null")
        self.assertEqual(row["status"], "failed")
        self.assertEqual(row["dispatch_state"], "failed")
        self.assertIn("target", row["dispatch_detail"].lower())
        self.assertIn("target", row["result_detail"].lower())
        db.close()

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
