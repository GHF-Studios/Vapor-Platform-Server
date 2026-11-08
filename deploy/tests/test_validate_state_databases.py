from __future__ import annotations

import importlib.util
import pathlib
import sqlite3
import tempfile
import unittest


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
SCRIPT = REPO_ROOT / "deploy/scripts/validate-state-databases.py"
SPEC = importlib.util.spec_from_file_location("validate_state_databases", SCRIPT)
validator = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(validator)
SNAPSHOT_SCRIPT = REPO_ROOT / "deploy/scripts/snapshot-state-databases.py"
SNAPSHOT_SPEC = importlib.util.spec_from_file_location("snapshot_state_databases", SNAPSHOT_SCRIPT)
snapshotter = importlib.util.module_from_spec(SNAPSHOT_SPEC)
assert SNAPSHOT_SPEC.loader is not None
SNAPSHOT_SPEC.loader.exec_module(snapshotter)


class ValidateStateDatabasesTests(unittest.TestCase):
    def make_state(self, root: pathlib.Path) -> None:
        for relative in validator.DATABASES:
            path = root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            connection = sqlite3.connect(path)
            try:
                connection.execute("CREATE TABLE state_probe (value TEXT)")
                connection.execute("INSERT INTO state_probe VALUES ('ok')")
                connection.commit()
            finally:
                connection.close()

    def test_accepts_consistent_required_databases(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            self.make_state(root)
            validator.validate(root)

    def test_rejects_missing_or_corrupt_required_database(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            self.make_state(root)
            (root / "registry/registry.sqlite3").write_bytes(b"not sqlite")
            with self.assertRaisesRegex(ValueError, "quick_check"):
                validator.validate(root)

    def test_snapshots_live_wal_databases_without_copying_live_journals(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            source = root / "source"
            destination = root / "snapshot"
            source.mkdir()
            for relative in validator.DATABASES:
                path = source / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                connection = sqlite3.connect(path)
                connection.execute("PRAGMA journal_mode=WAL")
                connection.execute("CREATE TABLE state_probe (value TEXT)")
                connection.execute("INSERT INTO state_probe VALUES ('captured')")
                connection.commit()
                if relative == validator.DATABASES[0]:
                    connection.execute("INSERT INTO state_probe VALUES ('wal-only')")
                    connection.commit()
                    self.assertTrue(path.with_name(path.name + "-wal").exists())
                connection.close()

            snapshotter.snapshot(source, destination)
            validator.validate(destination)
            for relative in validator.DATABASES:
                snapshot_path = destination / relative
                connection = sqlite3.connect(snapshot_path)
                try:
                    values = [row[0] for row in connection.execute("SELECT value FROM state_probe")]
                finally:
                    connection.close()
                self.assertIn("captured", values)
                self.assertFalse(snapshot_path.with_name(snapshot_path.name + "-wal").exists())
                self.assertFalse(snapshot_path.with_name(snapshot_path.name + "-shm").exists())
            connection = sqlite3.connect(destination / validator.DATABASES[0])
            try:
                values = [row[0] for row in connection.execute("SELECT value FROM state_probe")]
            finally:
                connection.close()
            self.assertIn("wal-only", values)


if __name__ == "__main__":
    unittest.main()
