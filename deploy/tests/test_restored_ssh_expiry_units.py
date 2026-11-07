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
PROVISIONING_ID = "3196a6b8-2eb2-4957-b56a-2b7241bfce7d"
FAILED_ID = "ed74751d-d3b2-4e67-a719-744121bd34d5"
REVOKED_ID = "f08c9dec-627f-414e-b4e5-54562100df2f"


class RestoredSshExpiryUnitsTests(unittest.TestCase):
    def test_lists_only_active_grants_and_requires_both_managed_units(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            database = root / "platform.sqlite3"
            unit_directory = root / "systemd"
            unit_directory.mkdir()
            with closing(sqlite3.connect(database)) as connection:
                connection.execute(
                    "CREATE TABLE platform_ssh_access_requests (id TEXT, status TEXT, expires_at_unix INTEGER)"
                )
                connection.executemany(
                    "INSERT INTO platform_ssh_access_requests VALUES (?,?,?)",
                    [
                        (REQUEST_ID, "active", 9999999999),
                        (PROVISIONING_ID, "provisioning", 9999999999),
                        (FAILED_ID, "failed", 9999999999),
                        (REVOKED_ID, "revoked", None),
                    ],
                )
                connection.commit()
            for request_id in (REQUEST_ID, PROVISIONING_ID, FAILED_ID):
                for suffix in ("timer", "service"):
                    (unit_directory / f"vapor-ssh-expire-{request_id}.{suffix}").write_text("[Unit]\n")

            result = subprocess.run(
                [sys.executable, str(SCRIPT), str(database), str(unit_directory)],
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(
                result.stdout.splitlines(),
                [
                    f"vapor-ssh-expire-{REQUEST_ID}.timer",
                    f"vapor-ssh-expire-{PROVISIONING_ID}.timer",
                    f"vapor-ssh-expire-{FAILED_ID}.timer",
                ],
            )

            (unit_directory / f"vapor-ssh-expire-{FAILED_ID}.service").unlink()
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
