from __future__ import annotations

import os
import pathlib
import subprocess
import tempfile
import unittest

from shell_test_support import bash_executable, posix_path, shell_path_list


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
DISPATCHER = REPO_ROOT / "deploy" / "scripts" / "vapor-ssh-deploy"


class VaporSshDeployTests(unittest.TestCase):
    def test_dispatch_reports_each_approved_uuid_it_queues(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            first = "11111111-1111-4111-8111-111111111111"
            second = "22222222-2222-4222-8222-222222222222"
            fake_bin = root / "bin"
            fake_bin.mkdir()
            capture = root / "systemctl.jsonl"
            (fake_bin / "systemctl").write_text(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$SYSTEMCTL_CAPTURE\"\n",
                encoding="utf-8",
            )
            (fake_bin / "python3").write_text(
                f"#!/bin/sh\nprintf '%s\\n' '{first}' '{second}'\n",
                encoding="utf-8",
            )
            for executable in (fake_bin / "systemctl", fake_bin / "python3"):
                executable.chmod(0o755)

            environment = os.environ.copy()
            environment["PATH"] = shell_path_list(fake_bin, environment["PATH"])
            environment["SYSTEMCTL_CAPTURE"] = str(capture)
            result = subprocess.run(
                [bash_executable(), posix_path(DISPATCHER)],
                check=False,
                env=environment,
                capture_output=True,
                text=True,
            )

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn(first, result.stdout)
            self.assertIn(second, result.stdout)
            queued = capture.read_text().splitlines()
            self.assertEqual(
                queued,
                [
                    f"start --no-block vapor-deploy@{first}.service",
                    f"start --no-block vapor-deploy@{second}.service",
                ],
            )


if __name__ == "__main__":
    unittest.main()
