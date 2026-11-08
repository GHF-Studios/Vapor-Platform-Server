from __future__ import annotations

import os
import pathlib
import re
import shutil
import subprocess
import tempfile
import unittest


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]


class StateOnlyRestoreRollbackTests(unittest.TestCase):
    def test_failed_preserve_move_does_not_delete_the_original_live_path(self) -> None:
        restore = (REPO_ROOT / "deploy/scripts/restore-state-only.sh").read_text()
        match = re.search(r"(?ms)^restore_previous\(\) \{\n.*?^\}", restore)
        self.assertIsNotNone(match, "restore script must expose its rollback function")

        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            state = root / "state"
            pre_restore = root / "pre-restore"
            target = state / "identity/identity.sqlite3"
            target.parent.mkdir(parents=True)
            target.write_text("original database", encoding="utf-8")

            bash = shutil.which("bash")
            if bash is None:
                git = shutil.which("git")
                if git is not None:
                    candidate = pathlib.Path(git).resolve().parents[1] / "usr/bin/bash.exe"
                    if candidate.is_file():
                        bash = str(candidate)
            if bash is None:
                self.skipTest("Bash is unavailable for the restore transaction test")
            def shell_path(path: pathlib.Path) -> str:
                converter = pathlib.Path(bash).parent / "cygpath.exe"
                if converter.is_file():
                    converted = subprocess.run(
                        [str(converter), "-u", str(path)], capture_output=True, text=True, check=True
                    )
                    return converted.stdout.strip()
                return str(path)

            state_shell = shell_path(state)
            pre_restore_shell = shell_path(pre_restore)
            target_shell = shell_path(target)
            script = "\n".join(
                [
                    match.group(0),
                    f"STATE_ROOT={state_shell!r}",
                    f"PRE_RESTORE={pre_restore_shell!r}",
                    "REPLACED_PATHS=(identity/identity.sqlite3)",
                    "PREEXISTING_PATHS=(identity/identity.sqlite3)",
                    "ROLLBACK_FAILED=false",
                    "restore_previous",
                    f"test -f {target_shell!r} && test \"$(cat {target_shell!r})\" = 'original database'",
                ]
            )
            environment = os.environ.copy()
            if pathlib.Path(bash).name.lower() == "bash.exe":
                environment["PATH"] = str(pathlib.Path(bash).parent) + os.pathsep + environment.get("PATH", "")
            completed = subprocess.run(
                [bash, "-c", script], capture_output=True, text=True, check=False, env=environment
            )

        self.assertEqual(
            completed.returncode,
            0,
            "rollback must preserve a live path when no pre-restore copy exists; "
            f"stdout={completed.stdout!r}, stderr={completed.stderr!r}",
        )


if __name__ == "__main__":
    unittest.main()
