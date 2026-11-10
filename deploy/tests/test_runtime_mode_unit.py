from __future__ import annotations

import pathlib
import unittest


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]


class RuntimeModeUnitTests(unittest.TestCase):
    def test_deployment_and_recovery_support_only_separate_service_processes(self) -> None:
        source = (REPO_ROOT / "src/main.rs").read_text()
        self.assertNotIn("vapor_identity_server::", source)
        self.assertNotIn('"monolith"', source)
        deployment = (REPO_ROOT / "deploy/scripts/deploy.sh").read_text()
        self.assertNotIn('= "monolith"', deployment)
        recovery = (REPO_ROOT / "deploy/scripts/restore-state.sh").read_text()
        self.assertNotIn("monolith)", recovery)


if __name__ == "__main__":
    unittest.main()
