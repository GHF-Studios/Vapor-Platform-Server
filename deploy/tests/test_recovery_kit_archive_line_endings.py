from __future__ import annotations

import pathlib
import subprocess
import tempfile
import unittest
import zipfile


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
RECOVERY_FILES = (
    "deploy/scripts/recover-state-only-host.sh",
    "deploy/scripts/restore-state-only.sh",
    "deploy/scripts/state_only_recovery_marker.py",
    "deploy/scripts/recovery_bundle_metadata.py",
    "deploy/scripts/state-bundle-validate.py",
    "deploy/scripts/deployed_source_manifest.py",
    "deploy/scripts/service_deploy_map.py",
    "deploy/scripts/validate-state-databases.py",
)


class RecoveryKitArchiveLineEndingTests(unittest.TestCase):
    def test_git_archive_keeps_linux_recovery_scripts_lf_on_windows(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            archive = pathlib.Path(temporary) / "recovery-kit.zip"
            recovery_commit = subprocess.check_output(
                [
                    "git",
                    "-C",
                    str(REPO_ROOT),
                    "log",
                    "-1",
                    "--format=%H",
                    "HEAD",
                    "--",
                    *RECOVERY_FILES,
                    ".gitattributes",
                ],
                text=True,
            ).strip()
            subprocess.run(
                [
                    "git",
                    "-C",
                    str(REPO_ROOT),
                    "archive",
                    "--format=zip",
                    f"--output={archive}",
                    recovery_commit,
                    *RECOVERY_FILES,
                ],
                check=True,
            )

            with zipfile.ZipFile(archive) as contents:
                for name in RECOVERY_FILES:
                    with self.subTest(name=name):
                        data = contents.read(name)
                        self.assertNotIn(b"\r", data)


if __name__ == "__main__":
    unittest.main()
