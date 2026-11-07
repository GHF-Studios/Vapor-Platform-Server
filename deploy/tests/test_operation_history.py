from __future__ import annotations

import importlib.util
import pathlib
import sqlite3
import stat
import tempfile
import unittest
from unittest import mock


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
HISTORY_MODULE = REPO_ROOT / "deploy" / "scripts" / "operation_history.py"
SPEC = importlib.util.spec_from_file_location("operation_history", HISTORY_MODULE)
HISTORY = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(HISTORY)


class OperationHistoryTests(unittest.TestCase):
    def test_refuses_root_writer_to_preserve_platform_database_ownership(self) -> None:
        if HISTORY.os.name != "posix" or not hasattr(HISTORY.os, "geteuid"):
            self.skipTest("root ownership is a Unix deployment concern")

        with tempfile.TemporaryDirectory() as temporary:
            database = pathlib.Path(temporary) / "platform.sqlite3"
            with mock.patch.object(HISTORY.os, "geteuid", return_value=0):
                with self.assertRaisesRegex(PermissionError, "Platform service account"):
                    HISTORY.record_operation(
                        database, "deploy", "success", "2026-11-01T10:00:00Z",
                        "2026-11-01T10:02:00Z", "a" * 40, "services",
                    )
            self.assertFalse(database.exists())

    def test_appends_multiple_operations_without_replacing_earlier_records(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            database = pathlib.Path(temporary) / "platform.sqlite3"
            earlier = HISTORY.record_operation(
                database, "deploy", "success", "2026-11-01T10:00:00Z",
                "2026-11-01T10:02:00Z", "a" * 40, "services",
            )
            later = HISTORY.record_operation(
                database, "restore", "failed", "2026-11-02T10:00:00Z",
                "2026-11-02T10:03:00Z", "b" * 40, "monolith",
            )

            self.assertNotEqual(earlier, later)
            connection = sqlite3.connect(database)
            try:
                rows = connection.execute(
                    "SELECT operation_id, operation, result FROM platform_operation_history ORDER BY finished_at_unix DESC"
                ).fetchall()
            finally:
                connection.close()
            self.assertEqual(rows, [(later, "restore", "failed"), (earlier, "deploy", "success")])
            if HISTORY.os.name == "posix":
                self.assertEqual(stat.S_IMODE(database.stat().st_mode), 0o640)
                self.assertEqual(database.stat().st_uid, HISTORY.os.geteuid())

    def test_rejects_invalid_operation_metadata_before_writing(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            database = pathlib.Path(temporary) / "platform.sqlite3"
            with self.assertRaises(ValueError):
                HISTORY.record_operation(
                    database, "shell", "success", "2026-11-01T10:00:00Z",
                    "2026-11-01T10:02:00Z", "a" * 40, "services",
                )
            self.assertFalse(database.exists())


if __name__ == "__main__":
    unittest.main()
