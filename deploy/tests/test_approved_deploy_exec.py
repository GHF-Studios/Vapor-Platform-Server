from __future__ import annotations

import os
import pathlib
import shlex
import shutil
import subprocess
import tempfile
import unittest


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
UNIT = REPO_ROOT / "deploy" / "systemd" / "vapor-deploy@.service"
WORKER = "/opt/vapor-platform-server/deploy/scripts/run-approved-deploy.sh"
WORKER_SCRIPT = REPO_ROOT / "deploy" / "scripts" / "run-approved-deploy.sh"
DEPLOY_SCRIPT = REPO_ROOT / "deploy" / "scripts" / "deploy.sh"
LIB_SCRIPT = REPO_ROOT / "deploy" / "scripts" / "lib.sh"
REQUEST_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "deploy.yml"


class ApprovedDeployExecTests(unittest.TestCase):
    def _bash(self) -> str:
        bash = shutil.which("bash") or r"C:\Program Files\Git\bin\bash.exe"
        if not pathlib.Path(bash).is_file():
            self.skipTest("Bash is required to exercise deploy environment loading")
        return bash

    def _load_server_mode(self, host_mode: str, request_mode: str | None, approved: bool) -> subprocess.CompletedProcess[str]:
        with tempfile.TemporaryDirectory() as directory:
            config = pathlib.Path(directory)
            (config / "platform.env").write_text(f"VAPOR_SERVER_MODE={host_mode}\n", encoding="utf-8")
            environment = os.environ.copy()
            environment.pop("VAPOR_SERVER_MODE", None)
            environment.pop("VAPOR_APPROVED_DEPLOY_ID", None)
            environment.update({"VAPOR_CONFIG_DIR": str(config), "LIB_SCRIPT": str(LIB_SCRIPT)})
            if request_mode is not None:
                environment["VAPOR_SERVER_MODE"] = request_mode
            if approved:
                environment["VAPOR_APPROVED_DEPLOY_ID"] = "17e1b3ee-d991-412a-aa41-eb0021d1837d"
            return subprocess.run(
                [self._bash(), "-c", 'source "$LIB_SCRIPT"; printf "%s" "$VAPOR_SERVER_MODE"'],
                env=environment,
                capture_output=True,
                text=True,
            )

    def test_approved_request_mode_survives_host_environment_loading(self) -> None:
        result = self._load_server_mode("monolith", "services", approved=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "services")

    def test_unapproved_context_uses_host_mode(self) -> None:
        result = self._load_server_mode("services", "monolith", approved=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "services")

    def test_approved_context_rejects_missing_request_mode(self) -> None:
        result = self._load_server_mode("services", None, approved=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("approved deployment request mode is missing or invalid", result.stderr)

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
