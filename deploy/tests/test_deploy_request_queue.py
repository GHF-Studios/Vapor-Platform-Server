from __future__ import annotations

import contextlib
import hashlib
import importlib.util
import io
import json
import pathlib
import subprocess
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
    def _database_with_request(self, request, status="approved", version=2, request_id="req"):
        db = sqlite3.connect(":memory:", isolation_level=None)
        db.row_factory = sqlite3.Row
        db.execute(
            "CREATE TABLE platform_deploy_requests ("
            "id TEXT PRIMARY KEY, status TEXT NOT NULL, version INTEGER NOT NULL, "
            "created_at_unix INTEGER NOT NULL DEFAULT 100, expires_at_unix INTEGER NOT NULL, decided_at_unix INTEGER, "
            "decided_by_profile_id TEXT, decision_reason TEXT, "
            "dispatch_state TEXT NOT NULL, dispatch_attempts INTEGER NOT NULL DEFAULT 1, "
            "dispatch_attempted_at_unix INTEGER, dispatch_detail TEXT, result_detail TEXT, "
            "body_hash TEXT NOT NULL, request_json TEXT NOT NULL, repository TEXT NOT NULL, "
            "branch TEXT NOT NULL, commit_hash TEXT NOT NULL, tree_hash TEXT NOT NULL, "
            "workflow_url TEXT NOT NULL, summary TEXT NOT NULL, submodules_json TEXT NOT NULL)"
        )
        raw = json.dumps(request, separators=(",", ":"))
        db.execute(
            "INSERT INTO platform_deploy_requests "
            "(id,status,version,expires_at_unix,decided_at_unix,dispatch_state,dispatch_attempts,"
            "dispatch_attempted_at_unix,dispatch_detail,result_detail,"
            "body_hash,request_json,repository,branch,commit_hash,tree_hash,workflow_url,summary,submodules_json) "
            "VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            (
                request_id, status, version, 9999999999, None,
                "not_attempted" if status == "pending" else "dispatching",
                0 if status == "pending" else 1, None if status == "pending" else 100, None, None,
                hashlib.sha256(raw.encode()).hexdigest(), raw,
                request["repository"], request["branch"], request["commit"], request["tree"],
                request["workflow_url"], request["summary"],
                json.dumps(request["submodules"], separators=(",", ":")),
            ),
        )
        return db

    def _request(self, server_mode="services", deployment_scope="full", service_target=None):
        request = {
            "server_mode": server_mode,
            "deployment_scope": deployment_scope,
            "service_target": service_target,
            "repository": "GHF-Studios/Vapor-Platform-Server",
            "branch": "main",
            "commit": "0" * 40,
            "tree": "1" * 40,
            "submodules": [],
            "workflow_url": "https://github.com/GHF-Studios/Vapor-Platform-Server/actions/runs/1",
            "summary": "CI passed",
        }
        return request

    def test_claim_preserves_explicit_deployment_target(self) -> None:
        db = self._database_with_request(self._request())
        output = io.StringIO()

        with mock.patch.object(QUEUE.time, "time", return_value=100), contextlib.redirect_stdout(output):
            result = QUEUE.claim(db, "req")

        self.assertEqual(result, 0)
        claimed = json.loads(output.getvalue())["request"]
        self.assertEqual(claimed["server_mode"], "services")
        self.assertEqual(claimed["deployment_scope"], "full")
        self.assertIsNone(claimed["service_target"])
        row = db.execute(
            "SELECT status, version, dispatch_state FROM platform_deploy_requests WHERE id='req'"
        ).fetchone()
        self.assertEqual(row["status"], "executing")
        self.assertEqual(row["dispatch_state"], "claimed")

        QUEUE.complete(db, "req", row["version"], "succeeded", "deployment ok")
        row = db.execute(
            "SELECT status, dispatch_state, result_detail FROM platform_deploy_requests WHERE id='req'"
        ).fetchone()
        self.assertEqual(row["status"], "succeeded")
        self.assertEqual(row["dispatch_state"], "completed")
        self.assertEqual(row["result_detail"], "deployment ok")
        db.close()

    def test_root_approval_records_reason_and_queues_only_the_fixed_request_unit(self) -> None:
        request = self._request()
        request_id = "596b4b19-f232-4528-bec8-cbc70c7398b9"
        db = self._database_with_request(request, status="pending", version=1, request_id=request_id)
        run = mock.Mock(return_value=subprocess.CompletedProcess([], 0))

        with mock.patch.object(QUEUE.time, "time", return_value=200), mock.patch.object(
            QUEUE.subprocess, "run", run
        ):
            result = QUEUE.approve_and_queue(
                db, request_id, request["commit"], "owner-authorized pre-alpha deploy", now=200
            )

        self.assertEqual(result, 0)
        run.assert_called_once_with(
            ["/usr/bin/systemctl", "start", "--no-block", f"vapor-deploy@{request_id}.service"],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
            timeout=5,
        )
        row = db.execute(
            "SELECT status,version,decided_by_profile_id,decided_at_unix,decision_reason,"
            "dispatch_state,dispatch_attempts,dispatch_attempted_at_unix "
            "FROM platform_deploy_requests WHERE id=?", (request_id,)
        ).fetchone()
        self.assertEqual(row["status"], "approved")
        self.assertEqual(row["version"], 2)
        self.assertIsNone(row["decided_by_profile_id"])
        self.assertEqual(row["decided_at_unix"], 200)
        self.assertIn("local root operator", row["decision_reason"].lower())
        self.assertIn("owner-authorized pre-alpha deploy", row["decision_reason"])
        self.assertEqual(row["dispatch_state"], "dispatching")
        self.assertEqual(row["dispatch_attempts"], 1)
        self.assertEqual(row["dispatch_attempted_at_unix"], 200)
        with self.assertRaisesRegex(ValueError, "only a pending"):
            QUEUE.approve_and_queue(db, request_id, request["commit"], "repeat approval", now=201)
        db.close()

    def test_root_approval_rejects_wrong_commit_without_mutation(self) -> None:
        request_id = "596b4b19-f232-4528-bec8-cbc70c7398b9"
        db = self._database_with_request(self._request(), status="pending", version=1, request_id=request_id)
        with self.assertRaisesRegex(ValueError, "commit"):
            QUEUE.approve_and_queue(db, request_id, "f" * 40, "approved by owner")
        row = db.execute(
            "SELECT status,version,decision_reason FROM platform_deploy_requests WHERE id=?", (request_id,)
        ).fetchone()
        self.assertEqual(tuple(row), ("pending", 1, None))
        db.close()

    def test_root_approval_rejects_non_official_request_without_mutation(self) -> None:
        request = self._request()
        request["branch"] = "codex/unreviewed"
        request_id = "596b4b19-f232-4528-bec8-cbc70c7398b9"
        db = self._database_with_request(request, status="pending", version=1, request_id=request_id)
        with self.assertRaisesRegex(ValueError, "main branch"):
            QUEUE.approve_and_queue(db, request_id, request["commit"], "approved by owner")
        row = db.execute(
            "SELECT status,version FROM platform_deploy_requests WHERE id=?", (request_id,)
        ).fetchone()
        self.assertEqual(tuple(row), ("pending", 1))
        db.close()

    def test_root_approval_rejects_expired_or_tampered_requests(self) -> None:
        request_id = "596b4b19-f232-4528-bec8-cbc70c7398b9"
        request = self._request()
        expired = self._database_with_request(request, status="pending", version=1, request_id=request_id)
        expired.execute("UPDATE platform_deploy_requests SET expires_at_unix=199 WHERE id=?", (request_id,))
        with self.assertRaisesRegex(ValueError, "expired"):
            QUEUE.approve_and_queue(expired, request_id, request["commit"], "approved deploy", now=200)
        self.assertEqual(
            tuple(expired.execute("SELECT status,version FROM platform_deploy_requests WHERE id=?", (request_id,)).fetchone()),
            ("pending", 1),
        )
        expired.close()

        tampered = self._database_with_request(request, status="pending", version=1, request_id=request_id)
        tampered.execute(
            "UPDATE platform_deploy_requests SET request_json=request_json || ' ' WHERE id=?", (request_id,)
        )
        with self.assertRaisesRegex(ValueError, "digest"):
            QUEUE.approve_and_queue(tampered, request_id, request["commit"], "approved deploy", now=200)
        self.assertEqual(
            tuple(tampered.execute("SELECT status,version FROM platform_deploy_requests WHERE id=?", (request_id,)).fetchone()),
            ("pending", 1),
        )
        tampered.close()

    def test_root_approval_worker_failure_preserves_decision_as_retryable(self) -> None:
        request = self._request()
        request_id = "596b4b19-f232-4528-bec8-cbc70c7398b9"
        db = self._database_with_request(request, status="pending", version=1, request_id=request_id)
        run = mock.Mock(return_value=subprocess.CompletedProcess([], 1))
        with mock.patch.object(QUEUE.time, "time", return_value=200), mock.patch.object(
            QUEUE.subprocess, "run", run
        ), self.assertRaisesRegex(RuntimeError, "approval was recorded"):
            QUEUE.approve_and_queue(
                db, request_id, request["commit"], "owner-authorized pre-alpha deploy", now=200
            )
        row = db.execute(
            "SELECT status,version,decision_reason,dispatch_state,dispatch_detail "
            "FROM platform_deploy_requests WHERE id=?", (request_id,)
        ).fetchone()
        self.assertEqual(row["status"], "approved")
        self.assertEqual(row["version"], 2)
        self.assertIsNotNone(row["decision_reason"])
        self.assertEqual(row["dispatch_state"], "failed")
        self.assertIn("retry", row["dispatch_detail"].lower())
        db.close()

    def test_claim_preserves_service_scope_and_target(self) -> None:
        request = self._request(deployment_scope="service", service_target="identity")
        db = self._database_with_request(request)
        output = io.StringIO()

        with mock.patch.object(QUEUE.time, "time", return_value=100), contextlib.redirect_stdout(output):
            result = QUEUE.claim(db, "req")

        self.assertEqual(result, 0)
        claimed = json.loads(output.getvalue())["request"]
        self.assertEqual(claimed["deployment_scope"], "service")
        self.assertEqual(claimed["service_target"], "identity")
        db.close()

    def test_claim_rejects_stale_or_unqueued_attempt(self) -> None:
        db = self._database_with_request(self._request())
        db.execute(
            "UPDATE platform_deploy_requests SET dispatch_state='queued', "
            "dispatch_attempted_at_unix=69 WHERE id='req'"
        )
        db.execute("UPDATE platform_deploy_requests SET dispatch_state='dispatching' WHERE id='req'")
        output = io.StringIO()
        with mock.patch.object(QUEUE.time, "time", return_value=100), contextlib.redirect_stdout(output):
            result = QUEUE.claim(db, "req")
        self.assertEqual(result, 0)
        self.assertEqual(output.getvalue().strip(), "null")
        row = db.execute(
            "SELECT status, dispatch_state FROM platform_deploy_requests WHERE id='req'"
        ).fetchone()
        self.assertEqual(row["status"], "approved")
        self.assertEqual(row["dispatch_state"], "dispatching")

        db.execute(
            "UPDATE platform_deploy_requests SET dispatch_state='not_attempted', "
            "dispatch_attempted_at_unix=100 WHERE id='req'"
        )
        output = io.StringIO()
        with mock.patch.object(QUEUE.time, "time", return_value=100), contextlib.redirect_stdout(output):
            result = QUEUE.claim(db, "req")
        self.assertEqual(result, 0)
        self.assertEqual(output.getvalue().strip(), "null")
        db.close()

    def test_queued_request_survives_waiting_for_global_deploy_lock(self) -> None:
        db = self._database_with_request(self._request())
        db.execute(
            "UPDATE platform_deploy_requests SET dispatch_state='queued', "
            "dispatch_attempted_at_unix=69 WHERE id='req'"
        )
        output = io.StringIO()
        with mock.patch.object(QUEUE.time, "time", return_value=100), contextlib.redirect_stdout(output):
            QUEUE.claim(db, "req")
        self.assertNotEqual(output.getvalue().strip(), "null")
        self.assertEqual(
            db.execute("SELECT status FROM platform_deploy_requests WHERE id='req'").fetchone()[0],
            "executing",
        )
        db.close()

    def test_newer_approved_request_cannot_overtake_older_open_request(self) -> None:
        db = self._database_with_request(self._request(), request_id="newer")
        request = self._request()
        raw = json.dumps(request, separators=(",", ":"))
        db.execute(
            "INSERT INTO platform_deploy_requests "
            "(id,status,version,created_at_unix,expires_at_unix,dispatch_state,dispatch_attempts,"
            "dispatch_attempted_at_unix,body_hash,request_json,repository,branch,commit_hash,tree_hash,"
            "workflow_url,summary,submodules_json) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            (
                "older", "pending", 1, 99, 9999999999, "not_attempted", 0, None,
                hashlib.sha256(raw.encode()).hexdigest(), raw, request["repository"], request["branch"],
                request["commit"], request["tree"], request["workflow_url"], request["summary"], "[]",
            ),
        )
        output = io.StringIO()
        with mock.patch.object(QUEUE.time, "time", return_value=100), contextlib.redirect_stdout(output):
            QUEUE.claim(db, "newer")
        self.assertEqual(output.getvalue().strip(), "null")
        self.assertEqual(
            tuple(db.execute("SELECT status,dispatch_state FROM platform_deploy_requests WHERE id='newer'").fetchone()),
            ("approved", "dispatching"),
        )
        db.close()

    def test_newer_request_cannot_be_approved_before_older_pending_request(self) -> None:
        request = self._request()
        newer_id = "596b4b19-f232-4528-bec8-cbc70c7398b9"
        older_id = "6d5d44f6-3c2c-4b5e-9668-a6edfe270204"
        db = self._database_with_request(request, status="pending", version=1, request_id=newer_id)
        db.execute("UPDATE platform_deploy_requests SET created_at_unix=200 WHERE id=?", (newer_id,))
        raw = json.dumps(request, separators=(",", ":"))
        db.execute(
            "INSERT INTO platform_deploy_requests "
            "(id,status,version,created_at_unix,expires_at_unix,dispatch_state,dispatch_attempts,"
            "body_hash,request_json,repository,branch,commit_hash,tree_hash,workflow_url,summary,submodules_json) "
            "VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            (
                older_id, "pending", 1, 100, 9999999999, "not_attempted", 0,
                hashlib.sha256(raw.encode()).hexdigest(), raw, request["repository"], request["branch"],
                request["commit"], request["tree"], request["workflow_url"], request["summary"], "[]",
            ),
        )
        with mock.patch.object(QUEUE.subprocess, "run") as run:
            with self.assertRaisesRegex(ValueError, "older deployment request"):
                QUEUE.approve_and_queue(db, newer_id, request["commit"], "newer revision", now=200)
        run.assert_not_called()
        self.assertEqual(
            tuple(db.execute("SELECT status,version FROM platform_deploy_requests WHERE id=?", (newer_id,)).fetchone()),
            ("pending", 1),
        )
        db.close()

    def test_next_ready_waits_for_oldest_open_request_then_returns_queued_successor(self) -> None:
        db = self._database_with_request(self._request(), request_id="newer")
        db.execute("UPDATE platform_deploy_requests SET dispatch_state='queued' WHERE id='newer'")
        request = self._request()
        raw = json.dumps(request, separators=(",", ":"))
        db.execute(
            "INSERT INTO platform_deploy_requests "
            "(id,status,version,created_at_unix,expires_at_unix,dispatch_state,dispatch_attempts,"
            "dispatch_attempted_at_unix,body_hash,request_json,repository,branch,commit_hash,tree_hash,"
            "workflow_url,summary,submodules_json) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            (
                "older", "pending", 1, 99, 9999999999, "not_attempted", 0, None,
                hashlib.sha256(raw.encode()).hexdigest(), raw, request["repository"], request["branch"],
                request["commit"], request["tree"], request["workflow_url"], request["summary"], "[]",
            ),
        )
        output = io.StringIO()
        with mock.patch.object(QUEUE.time, "time", return_value=100), contextlib.redirect_stdout(output):
            QUEUE.next_ready(db)
        self.assertEqual(output.getvalue().strip(), "null")
        db.execute("UPDATE platform_deploy_requests SET status='succeeded' WHERE id='older'")
        output = io.StringIO()
        with mock.patch.object(QUEUE.time, "time", return_value=100), contextlib.redirect_stdout(output):
            QUEUE.next_ready(db)
        self.assertEqual(output.getvalue().strip(), "newer")
        db.close()

    def test_claim_fails_closed_for_invalid_scope_target_combinations(self) -> None:
        invalid_requests = [
            self._request(deployment_scope="service", service_target=None),
            self._request(deployment_scope="full", service_target="identity"),
            self._request(deployment_scope="service", service_target="unknown"),
            self._request("monolith", deployment_scope="service", service_target="identity"),
            self._request(deployment_scope="partial", service_target=None),
        ]
        for index, request in enumerate(invalid_requests):
            with self.subTest(index=index):
                db = self._database_with_request(request)
                output = io.StringIO()
                with mock.patch.object(QUEUE.time, "time", return_value=100), contextlib.redirect_stdout(output):
                    result = QUEUE.claim(db, "req")
                row = db.execute(
                    "SELECT status, dispatch_state, dispatch_detail FROM platform_deploy_requests WHERE id='req'"
                ).fetchone()
                self.assertEqual(result, 0)
                self.assertEqual(output.getvalue().strip(), "null")
                self.assertEqual(row["status"], "failed")
                self.assertEqual(row["dispatch_state"], "failed")
                self.assertIn("scope", row["dispatch_detail"].lower())
                db.close()

    def test_verify_rejects_target_different_from_approved_request(self) -> None:
        request = self._request()
        db = self._database_with_request(request, status="executing", version=3)

        with self.assertRaisesRegex(ValueError, "topology"):
            QUEUE.verify_claim(db, "req", 3, request["commit"], "monolith", "full", None)

        db.close()

    def test_verify_rejects_scope_or_target_different_from_approved_request(self) -> None:
        request = self._request(deployment_scope="service", service_target="identity")
        db = self._database_with_request(request, status="executing", version=3)

        with self.assertRaisesRegex(ValueError, "scope"):
            QUEUE.verify_claim(db, "req", 3, request["commit"], "services", "full", None)
        with self.assertRaisesRegex(ValueError, "target"):
            QUEUE.verify_claim(db, "req", 3, request["commit"], "services", "service", "registry")

        db.close()

    def test_approved_legacy_request_without_target_is_failed_closed(self) -> None:
        db = sqlite3.connect(":memory:", isolation_level=None)
        db.row_factory = sqlite3.Row
        db.execute(
            "CREATE TABLE platform_deploy_requests ("
            "id TEXT PRIMARY KEY, status TEXT NOT NULL, version INTEGER NOT NULL, created_at_unix INTEGER NOT NULL DEFAULT 100, "
            "expires_at_unix INTEGER NOT NULL, decided_at_unix INTEGER, "
            "dispatch_state TEXT NOT NULL, dispatch_attempts INTEGER NOT NULL DEFAULT 1, "
            "dispatch_attempted_at_unix INTEGER, dispatch_detail TEXT, result_detail TEXT, "
            "body_hash TEXT NOT NULL, request_json TEXT NOT NULL, repository TEXT NOT NULL, "
            "branch TEXT NOT NULL, commit_hash TEXT NOT NULL, tree_hash TEXT NOT NULL, "
            "workflow_url TEXT NOT NULL, summary TEXT NOT NULL, submodules_json TEXT NOT NULL)"
        )
        request = {key: value for key, value in self._request().items() if key != "deployment_scope" and key != "service_target"}
        request["summary"] = "old request without target"
        raw = json.dumps(request, separators=(",", ":"))
        db.execute(
            "INSERT INTO platform_deploy_requests "
            "(id,status,version,expires_at_unix,decided_at_unix,dispatch_state,dispatch_attempts,"
            "dispatch_attempted_at_unix,dispatch_detail,result_detail,"
            "body_hash,request_json,repository,branch,commit_hash,tree_hash,workflow_url,summary,submodules_json) "
            "VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            (
                "legacy", "approved", 2, 9999999999, None, "queued", 1, 100, None, None,
                hashlib.sha256(raw.encode()).hexdigest(), raw, request["repository"],
                request["branch"], request["commit"], request["tree"],
                request["workflow_url"], request["summary"], "[]",
            ),
        )
        output = io.StringIO()

        with mock.patch.object(QUEUE.time, "time", return_value=100), contextlib.redirect_stdout(output):
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
            "id TEXT PRIMARY KEY, status TEXT NOT NULL, version INTEGER NOT NULL, created_at_unix INTEGER NOT NULL DEFAULT 100, "
            "expires_at_unix INTEGER NOT NULL, decided_at_unix INTEGER, "
            "dispatch_state TEXT NOT NULL, dispatch_attempted_at_unix INTEGER, dispatch_detail TEXT)"
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
