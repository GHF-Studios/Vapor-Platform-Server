from __future__ import annotations

import pathlib
import shlex
import unittest


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
UNIT = REPO_ROOT / "deploy" / "systemd" / "vapor-deploy@.service"
WORKER = "/opt/vapor-platform-server/deploy/scripts/run-approved-deploy.sh"
WORKER_SCRIPT = REPO_ROOT / "deploy" / "scripts" / "run-approved-deploy.sh"
DEPLOY_SCRIPT = REPO_ROOT / "deploy" / "scripts" / "deploy.sh"
REQUEST_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "deploy.yml"


class ApprovedDeployExecTests(unittest.TestCase):
    def test_non_executable_worker_is_invoked_through_bash(self) -> None:
        lines = UNIT.read_text(encoding="utf-8").splitlines()
        exec_line = next(line for line in lines if line.startswith("ExecStart="))
        command = shlex.split(exec_line.removeprefix("ExecStart="))

        worker_index = command.index(WORKER)
        self.assertGreater(worker_index, 0)
        self.assertEqual(command[worker_index - 1], "/usr/bin/bash")

    def test_approved_worker_passes_request_target_into_deploy(self) -> None:
        worker = WORKER_SCRIPT.read_text(encoding="utf-8")
        deploy = DEPLOY_SCRIPT.read_text(encoding="utf-8")

        self.assertIn('REQUEST_MODE="$(python3', worker)
        self.assertIn('VAPOR_SERVER_MODE="${REQUEST_MODE}"', worker)
        self.assertIn('--server-mode "${VAPOR_SERVER_MODE}"', deploy)

    def test_ci_request_targets_independent_services_explicitly(self) -> None:
        workflow = REQUEST_WORKFLOW.read_text(encoding="utf-8")

        self.assertIn('"server_mode": "services"', workflow)

    def test_manual_target_workflow_is_main_only_and_emits_typed_scope(self) -> None:
        workflow = REQUEST_WORKFLOW.read_text(encoding="utf-8")

        self.assertIn("workflow_dispatch:", workflow)
        self.assertIn("atlas", workflow)
        self.assertIn("build-cache", workflow)
        self.assertIn("github.ref == 'refs/heads/main'", workflow)
        self.assertIn('"deployment_scope": scope', workflow)
        self.assertIn('"service_target": None if scope == "full" else target', workflow)

    def test_scoped_worker_uses_isolated_clone_and_only_checks_out_target_submodule(self) -> None:
        worker = WORKER_SCRIPT.read_text(encoding="utf-8")

        full_branch = worker.index('if [ "${REQUEST_SCOPE}" = "full" ]; then')
        checkout = worker.index('git -C "${VAPOR_DEPLOY_ROOT}" checkout --detach --force')
        scoped_clone = worker.index('git clone --no-checkout --no-hardlinks')
        self.assertLess(full_branch, checkout)
        self.assertLess(checkout, scoped_clone)
        self.assertIn('git -C "${REQUEST_ROOT}" submodule update --init -- "${REQUEST_SUBMODULE_PATH}"', worker)
        self.assertIn('scope == "full" or item["path"] == target_path', worker)
        self.assertNotIn('worktree add', worker)
        self.assertIn('VAPOR_APPROVED_SOURCE_ROOT="${REQUEST_ROOT}"', worker)


if __name__ == "__main__":
    unittest.main()
