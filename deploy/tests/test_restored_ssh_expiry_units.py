from __future__ import annotations

import pathlib
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from contextlib import closing


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
SCRIPT = REPO_ROOT / "deploy/scripts/restored_ssh_expiry_units.py"
REQUEST_ID = "240aab1d-8edc-49bd-bbd1-47706983e2c3"


class RestoredSshExpiryUnitsTests(unittest.TestCase):
    def test_lists_only_active_grants_and_requires_both_managed_units(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            database = root / "platform.sqlite3"
            unit_directory = root / "systemd"
            unit_directory.mkdir()
            with closing(sqlite3.connect(database)) as connection:
                connection.execute(
                    "CREATE TABLE platform_ssh_access_requests (id TEXT, status TEXT)"
                )
                connection.executemany(
                    "INSERT INTO platform_ssh_access_requests VALUES (?,?)",
                    [(REQUEST_ID, "active"), ("ed74751d-d3b2-4e67-a719-744121bd34d5", "revoked")],
                )
                connection.commit()
            for suffix in ("timer", "service"):
                (unit_directory / f"vapor-ssh-expire-{REQUEST_ID}.{suffix}").write_text("[Unit]\n")

            result = subprocess.run(
                [sys.executable, str(SCRIPT), str(database), str(unit_directory)],
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout.strip(), f"vapor-ssh-expire-{REQUEST_ID}.timer")

            (unit_directory / f"vapor-ssh-expire-{REQUEST_ID}.service").unlink()
            missing = subprocess.run(
                [sys.executable, str(SCRIPT), str(database), str(unit_directory)],
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(missing.returncode, 1)
            self.assertIn("missing its managed expiry unit", missing.stderr)


if __name__ == "__main__":
    unittest.main()
