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
    def test_lists_recoverable_grants_and_requires_both_managed_units(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            database = root / "platform.sqlite3"
            unit_directory = root / "systemd"
            unit_directory.mkdir()
            with closing(sqlite3.connect(database)) as connection:
                connection.execute(
                    "CREATE TABLE platform_ssh_access_requests "
                    "(id TEXT, status TEXT, expires_at_unix INTEGER, operation_detail TEXT)"
                )
                connection.executemany(
                    "INSERT INTO platform_ssh_access_requests VALUES (?,?,?,?)",
                    [
                        (REQUEST_ID, "active", 9999999999, None),
                        (PROVISIONING_ID, "provisioning", 9999999999, "granting"),
                        (FAILED_ID, "failed", 9999999999, "expiry cleanup failed; access may remain active"),
                        (REVOKED_ID, "revoked", None, None),
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

    def test_skips_cleaned_failed_grants_but_keeps_uncertain_access_states(self) -> None:
        active_id = REQUEST_ID
        clean_grant_id = "ed74751d-d3b2-4e67-a719-744121bd34d5"
        failed_revoke_id = "e304fa3d-6d3d-44c9-9680-c97f6538de57"
        interrupted_id = "bf915d54-655f-4144-9abc-07f6916a378b"
        incomplete_grant_id = "da10fd21-ab4f-41c5-acdc-78c77e5a3124"
        unknown_failure_id = "3d0cd537-4046-44dc-bcd2-8ef4384459ef"
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            database = root / "platform.sqlite3"
            unit_directory = root / "systemd"
            unit_directory.mkdir()
            with closing(sqlite3.connect(database)) as connection:
                connection.execute(
                    "CREATE TABLE platform_ssh_access_requests "
                    "(id TEXT, status TEXT, expires_at_unix INTEGER, operation_detail TEXT)"
                )
                connection.executemany(
                    "INSERT INTO platform_ssh_access_requests VALUES (?,?,?,?)",
                    [
                        (active_id, "active", 500, None),
                        (clean_grant_id, "failed", 500, "grant helper failed: helper reported failure: could not create temporary account"),
                        (failed_revoke_id, "failed", 500, "revocation helper failed; access may remain active"),
                        (interrupted_id, "provisioning", 500, "granting"),
                        (incomplete_grant_id, "failed", 500, "grant helper failed: grant setup failed and rollback was incomplete; access artifacts may remain"),
                        (unknown_failure_id, "failed", 500, "grant helper failed: privileged helper timed out"),
                    ],
                )
                connection.commit()
            for request_id in (active_id, failed_revoke_id, interrupted_id, incomplete_grant_id):
                for suffix in ("timer", "service"):
                    (unit_directory / f"vapor-ssh-expire-{request_id}.{suffix}").write_text("[Unit]\n")

            result = subprocess.run(
                [sys.executable, str(SCRIPT), str(database), str(unit_directory)],
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(result.returncode, 1)
            self.assertIn("missing its managed expiry unit", result.stderr)
            for suffix in ("timer", "service"):
                (unit_directory / f"vapor-ssh-expire-{unknown_failure_id}.{suffix}").write_text("[Unit]\n")

            result = subprocess.run(
                [sys.executable, str(SCRIPT), str(database), str(unit_directory)],
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(
                result.stdout.strip().splitlines(),
                [
                    f"vapor-ssh-expire-{active_id}.timer",
                    f"vapor-ssh-expire-{unknown_failure_id}.timer",
                    f"vapor-ssh-expire-{interrupted_id}.timer",
                    f"vapor-ssh-expire-{incomplete_grant_id}.timer",
                    f"vapor-ssh-expire-{failed_revoke_id}.timer",
                ],
            )


if __name__ == "__main__":
    unittest.main()
