from __future__ import annotations

import pathlib
import unittest


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
UNIT = REPO_ROOT / "deploy" / "systemd" / "vapor-deploy@.service"


class ApprovedDeployUnitTests(unittest.TestCase):
    def test_worker_waits_for_shared_deploy_lock_and_has_no_build_timeout(self) -> None:
        text = UNIT.read_text(encoding="utf-8")
        self.assertIn("TimeoutStartSec=infinity", text)
        self.assertIn(
            "ExecStart=/usr/bin/flock /run/vapor-server-deploy.lock ", text
        )
        self.assertNotIn("flock -n", text)


if __name__ == "__main__":
    unittest.main()
