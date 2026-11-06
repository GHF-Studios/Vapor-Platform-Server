from __future__ import annotations

import pathlib
import shlex
import unittest


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
UNIT = REPO_ROOT / "deploy" / "systemd" / "vapor-deploy@.service"
WORKER = "/opt/vapor-platform-server/deploy/scripts/run-approved-deploy.sh"


class ApprovedDeployExecTests(unittest.TestCase):
    def test_non_executable_worker_is_invoked_through_bash(self) -> None:
        lines = UNIT.read_text(encoding="utf-8").splitlines()
        exec_line = next(line for line in lines if line.startswith("ExecStart="))
        command = shlex.split(exec_line.removeprefix("ExecStart="))

        worker_index = command.index(WORKER)
        self.assertGreater(worker_index, 0)
        self.assertEqual(command[worker_index - 1], "/usr/bin/bash")


if __name__ == "__main__":
    unittest.main()
