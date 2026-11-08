from __future__ import annotations

import importlib.util
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
MARKER_TOOL = REPO_ROOT / "deploy/scripts/state_only_recovery_marker.py"
MARKER_SPEC = importlib.util.spec_from_file_location("state_only_recovery_marker", MARKER_TOOL)
assert MARKER_SPEC is not None and MARKER_SPEC.loader is not None
MARKER_MODULE = importlib.util.module_from_spec(MARKER_SPEC)
MARKER_SPEC.loader.exec_module(MARKER_MODULE)
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

    def test_marker_writer_works_when_descriptor_chmod_is_unavailable(self) -> None:
        value = {
            "schema_version": 1,
            "bundle_sha256": BUNDLE_DIGEST,
            "root_commit": ROOT_COMMIT,
            "phase": "building",
        }
        with mock.patch.object(MARKER_MODULE.os, "fchmod", None, create=True):
            MARKER_MODULE._write(self.root, value, create=True)
        self.assertEqual(json.loads((self.root / "state-only.json").read_text()), value)

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

    def test_read_recovers_from_a_stale_atomic_write_temporary_file(self) -> None:
        self.init()
        interrupted_write = self.root / ".state-only.abcd1234"
        interrupted_write.write_text('{"partial":', encoding="utf-8")
        result = self.run_tool(
            "read",
            "--root",
            str(self.root),
            "--bundle-sha256",
            BUNDLE_DIGEST,
            "--root-commit",
            ROOT_COMMIT,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(interrupted_write.exists())
        self.assertEqual(json.loads(result.stdout)["phase"], "building")

    def test_prepare_cleans_interrupted_first_write_and_allows_init(self) -> None:
        self.root.mkdir()
        interrupted_write = self.root / ".state-only.abcd1234"
        interrupted_write.write_text('{"partial":', encoding="utf-8")
        prepared = self.run_tool(
            "prepare",
            "--root",
            str(self.root),
            "--bundle-sha256",
            BUNDLE_DIGEST,
            "--root-commit",
            ROOT_COMMIT,
        )
        self.assertEqual(prepared.returncode, 0, prepared.stderr)
        self.assertFalse(interrupted_write.exists())
        initialized = self.run_tool(
            "init",
            "--root",
            str(self.root),
            "--bundle-sha256",
            BUNDLE_DIGEST,
            "--root-commit",
            ROOT_COMMIT,
        )
        self.assertEqual(initialized.returncode, 0, initialized.stderr)

    def test_prepare_rejects_unknown_or_symlink_entries(self) -> None:
        self.root.mkdir()
        unknown = self.root / ".state-only.not-a-temp"
        unknown.write_text("preserve", encoding="utf-8")
        result = self.run_tool(
            "prepare",
            "--root",
            str(self.root),
            "--bundle-sha256",
            BUNDLE_DIGEST,
            "--root-commit",
            ROOT_COMMIT,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(unknown.read_text(encoding="utf-8"), "preserve")

        unknown.unlink()
        link = self.root / ".state-only.abcd1234"
        target = self.root.parent / "target"
        target.write_text("preserve", encoding="utf-8")
        try:
            link.symlink_to(target)
        except (OSError, NotImplementedError):
            self.skipTest("symlinks are unavailable")
        result = self.run_tool(
            "prepare",
            "--root",
            str(self.root),
            "--bundle-sha256",
            BUNDLE_DIGEST,
            "--root-commit",
            ROOT_COMMIT,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue(link.is_symlink())

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
