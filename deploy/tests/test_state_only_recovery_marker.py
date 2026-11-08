from __future__ import annotations

import json
import os
import pathlib
import subprocess
import sys
import tempfile
import unittest


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
MARKER_TOOL = REPO_ROOT / "deploy/scripts/state_only_recovery_marker.py"
BUNDLE_DIGEST = "a" * 64
ROOT_COMMIT = "b" * 40


class StateOnlyRecoveryMarkerTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = pathlib.Path(self.temp.name) / "recovery-marker"

    def run_tool(self, *args: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(MARKER_TOOL), *args],
            capture_output=True,
            text=True,
            check=False,
        )

    def init(self) -> None:
        result = self.run_tool(
            "init",
            "--root",
            str(self.root),
            "--bundle-sha256",
            BUNDLE_DIGEST,
            "--root-commit",
            ROOT_COMMIT,
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_init_writes_a_small_secret_free_marker(self) -> None:
        result = self.run_tool(
            "init",
            "--root",
            str(self.root),
            "--bundle-sha256",
            BUNDLE_DIGEST,
            "--root-commit",
            ROOT_COMMIT,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        marker = json.loads((self.root / "state-only.json").read_text())
        self.assertEqual(
            marker,
            {
                "schema_version": 1,
                "bundle_sha256": BUNDLE_DIGEST,
                "root_commit": ROOT_COMMIT,
                "phase": "building",
            },
        )
        self.assertNotIn("secret", result.stdout.lower())
        if os.name == "posix":
            self.assertEqual((self.root.stat().st_mode & 0o777), 0o700)
            self.assertEqual(((self.root / "state-only.json").stat().st_mode & 0o777), 0o600)

    def test_read_rejects_a_different_bundle_or_source_revision(self) -> None:
        self.init()
        for flag, value in (
            ("--bundle-sha256", "c" * 64),
            ("--root-commit", "d" * 40),
        ):
            with self.subTest(flag=flag):
                result = self.run_tool(
                    "read",
                    "--root",
                    str(self.root),
                    "--bundle-sha256",
                    value if flag == "--bundle-sha256" else BUNDLE_DIGEST,
                    "--root-commit",
                    value if flag == "--root-commit" else ROOT_COMMIT,
                )
                self.assertNotEqual(result.returncode, 0)

    def test_phase_advance_and_completion_are_exact(self) -> None:
        self.init()
        updated = self.run_tool(
            "set-phase",
            "--root",
            str(self.root),
            "--bundle-sha256",
            BUNDLE_DIGEST,
            "--root-commit",
            ROOT_COMMIT,
            "--phase",
            "awaiting_provider_checks",
        )
        self.assertEqual(updated.returncode, 0, updated.stderr)
        read = self.run_tool(
            "read",
            "--root",
            str(self.root),
            "--bundle-sha256",
            BUNDLE_DIGEST,
            "--root-commit",
            ROOT_COMMIT,
        )
        self.assertEqual(read.returncode, 0, read.stderr)
        self.assertEqual(json.loads(read.stdout)["phase"], "awaiting_provider_checks")
        complete = self.run_tool(
            "complete",
            "--root",
            str(self.root),
            "--bundle-sha256",
            BUNDLE_DIGEST,
            "--root-commit",
            ROOT_COMMIT,
        )
        self.assertEqual(complete.returncode, 0, complete.stderr)
        self.assertFalse(self.root.exists())

    def test_init_refuses_to_adopt_an_unrelated_existing_directory(self) -> None:
        self.root.mkdir()
        unrelated = self.root / "operator-data"
        unrelated.write_text("preserve")
        result = self.run_tool(
            "init",
            "--root",
            str(self.root),
            "--bundle-sha256",
            BUNDLE_DIGEST,
            "--root-commit",
            ROOT_COMMIT,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(unrelated.read_text(), "preserve")

    def test_read_rejects_unknown_marker_fields(self) -> None:
        self.init()
        marker_path = self.root / "state-only.json"
        marker = json.loads(marker_path.read_text())
        marker["provider_secret"] = "must not be accepted"
        marker_path.write_text(json.dumps(marker))
        result = self.run_tool(
            "read",
            "--root",
            str(self.root),
            "--bundle-sha256",
            BUNDLE_DIGEST,
            "--root-commit",
            ROOT_COMMIT,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn("must not be accepted", result.stderr)


if __name__ == "__main__":
    unittest.main()
