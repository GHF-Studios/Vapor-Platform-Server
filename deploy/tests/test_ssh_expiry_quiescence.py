from __future__ import annotations

import pathlib
import os
import shlex
import subprocess
import tempfile
import unittest

from shell_test_support import bash_executable, posix_path, shell_path_list


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
HELPER = REPO_ROOT / "deploy/scripts/ssh-expiry-quiescence.sh"


class SshExpiryQuiescenceTests(unittest.TestCase):
    def test_pauses_active_timers_drains_queued_work_then_restarts_original_timers(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            fake_bin = root / "bin"
            fake_bin.mkdir()
            calls = root / "calls.log"
            systemctl = fake_bin / "systemctl"
            systemctl.write_text(
                "#!/usr/bin/env bash\n"
                "printf '%s\\n' \"$*\" >> \"$FAKE_SYSTEMCTL_LOG\"\n"
                "case \"$1 $2\" in\n"
                "  'list-units --type=timer')\n"
                "    printf '%s\\n' 'vapor-ssh-expire-a.timer loaded active waiting' 'vapor-ssh-expire-b.timer loaded active waiting' ;;\n"
                "  'stop vapor-ssh-expire-a.timer'|'stop vapor-ssh-expire-b.timer'|'start vapor-ssh-expire-a.timer'|'start vapor-ssh-expire-b.timer') exit 0 ;;\n"
                "  'list-units --type=service')\n"
                "    n=0; [ ! -f \"$FAKE_SERVICE_POLLS\" ] || n=$(cat \"$FAKE_SERVICE_POLLS\"); n=$((n + 1)); printf '%s' \"$n\" > \"$FAKE_SERVICE_POLLS\";\n"
                "    [ \"$n\" -gt 1 ] || printf '%s\\n' 'vapor-ssh-expire-a.service loaded activating start' ;;\n"
                "  'list-jobs --no-legend')\n"
                "    n=0; [ ! -f \"$FAKE_JOB_POLLS\" ] || n=$(cat \"$FAKE_JOB_POLLS\"); n=$((n + 1)); printf '%s' \"$n\" > \"$FAKE_JOB_POLLS\";\n"
                "    [ \"$n\" -gt 1 ] || printf '%s\\n' '3 vapor-ssh-expire-b.service start waiting' ;;\n"
                "  *) exit 2 ;;\n"
                "esac\n",
                encoding="utf-8",
            )
            systemctl.chmod(0o755)
            script = root / "run.sh"
            script.write_text(
                "#!/usr/bin/env bash\nset -euo pipefail\n"
                f"source {shlex.quote(posix_path(HELPER))}\n"
                "ACTIVE_SSH_EXPIRY_TIMERS=()\n"
                "pause_temporary_ssh_expiry\n"
                "restart_temporary_ssh_expiry\n"
                "printf 'timers=%s\\n' \"${ACTIVE_SSH_EXPIRY_TIMERS[*]}\"\n",
                encoding="utf-8",
            )
            environment = {
                "PATH": shell_path_list(fake_bin, os.environ["PATH"]),
                "FAKE_SYSTEMCTL_LOG": posix_path(calls),
                "FAKE_SERVICE_POLLS": posix_path(root / "service-polls"),
                "FAKE_JOB_POLLS": posix_path(root / "job-polls"),
            }
            result = subprocess.run(
                [bash_executable(), posix_path(script)],
                capture_output=True,
                text=True,
                env={**os.environ, **environment},
                check=False,
            )

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(
                result.stdout.strip(),
                "timers=vapor-ssh-expire-a.timer vapor-ssh-expire-b.timer",
            )
            observed = calls.read_text(encoding="utf-8").splitlines()
            self.assertEqual(
                observed,
                [
                    "list-units --type=timer --state=active --no-legend vapor-ssh-expire-*.timer",
                    "stop vapor-ssh-expire-a.timer",
                    "stop vapor-ssh-expire-b.timer",
                    "list-units --type=service --state=active,activating,deactivating --no-legend vapor-ssh-expire-*.service",
                    "list-jobs --no-legend",
                    "list-units --type=service --state=active,activating,deactivating --no-legend vapor-ssh-expire-*.service",
                    "list-jobs --no-legend",
                    "start vapor-ssh-expire-a.timer",
                    "start vapor-ssh-expire-b.timer",
                ],
            )

    def test_inspection_failure_aborts_before_any_timer_is_restarted(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            fake_bin = root / "bin"
            fake_bin.mkdir()
            (fake_bin / "systemctl").write_text(
                "#!/usr/bin/env bash\nexit 1\n", encoding="utf-8"
            )
            (fake_bin / "systemctl").chmod(0o755)
            script = root / "run.sh"
            script.write_text(
                "#!/usr/bin/env bash\nset -euo pipefail\n"
                f"source {shlex.quote(posix_path(HELPER))}\n"
                "ACTIVE_SSH_EXPIRY_TIMERS=()\n"
                "pause_temporary_ssh_expiry\n",
                encoding="utf-8",
            )
            result = subprocess.run(
                [bash_executable(), posix_path(script)],
                capture_output=True,
                text=True,
                env={
                    **os.environ,
                    "PATH": shell_path_list(fake_bin, os.environ["PATH"]),
                },
                check=False,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("could not inspect temporary SSH expiry timers", result.stderr)


if __name__ == "__main__":
    unittest.main()
