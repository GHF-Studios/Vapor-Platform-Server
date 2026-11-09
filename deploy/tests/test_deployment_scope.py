from __future__ import annotations

import importlib.util
import pathlib
import sys
import unittest


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
SCRIPT_PATH = REPO_ROOT / "deploy" / "scripts" / "deployment_scope.py"
sys.path.insert(0, str(SCRIPT_PATH.parent))
SPEC = importlib.util.spec_from_file_location("deployment_scope", SCRIPT_PATH)
SCOPE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(SCOPE)


class DeploymentScopeTests(unittest.TestCase):
    def test_documentation_and_workflow_only_changes_do_not_request_deployment(self) -> None:
        self.assertEqual(
            SCOPE.classify_changed_paths(
                [
                    "docs/deployment-status.md",
                    "README.md",
                    "new-ops-note.md",
                    ".github/workflows/deploy.yml",
                    "deploy/tests/test_deployment_scope.py",
                    "deploy/scripts/deployment_scope.py",
                ]
            ),
            "none",
        )

    def test_offline_recovery_tool_changes_do_not_deploy_live_services(self) -> None:
        self.assertEqual(
            SCOPE.classify_changed_paths(
                [
                    "deploy/scripts/README-state-only-recovery.txt",
                    "deploy/scripts/README.txt",
                    "deploy/scripts/recover-state-only-host.sh",
                    "deploy/scripts/recover-fresh-host.sh",
                    "deploy/scripts/backup-state-only-to-usb.ps1",
                    "deploy/scripts/test.sh",
                    "deploy/tests/test_state_only_fresh_host_contract.py",
                ]
            ),
            "none",
        )

    def test_explicit_platform_server_sources_select_platform_target(self) -> None:
        self.assertEqual(
            SCOPE.classify_changed_paths(
                [
                    "src/main.rs",
                    "src/admin.rs",
                    "src/deploy_requests.rs",
                    "src/github_oidc.rs",
                ]
            ),
            "platform",
        )

    def test_ops_shared_and_unknown_source_changes_stay_full(self) -> None:
        for path in (
            "src/bin/vapor_platform_ops.rs",
            "src/recovery_status.rs",
            "src/ssh_policy.rs",
            "src/ssh_cli.rs",
            "src/unclassified_module.rs",
        ):
            with self.subTest(path=path):
                self.assertEqual(SCOPE.classify_changed_paths([path]), "full")

    def test_platform_source_and_service_change_select_full_composition(self) -> None:
        self.assertEqual(
            SCOPE.classify_changed_paths(["src/admin.rs", "Vapor-Identity-Server"]),
            "full",
        )

    def test_one_service_submodule_change_selects_only_that_service(self) -> None:
        self.assertEqual(
            SCOPE.classify_changed_paths(["Vapor-Identity-Server"]),
            "identity",
        )

    def test_static_site_submodule_selects_atlas(self) -> None:
        self.assertEqual(SCOPE.classify_changed_paths(["Atlas"]), "atlas")

    def test_service_submodule_and_unrelated_root_docs_stay_scoped(self) -> None:
        self.assertEqual(
            SCOPE.classify_changed_paths(
                ["Vapor-Docs-Server", "docs/operations.md", "README.md"]
            ),
            "docs",
        )

    def test_multiple_service_submodules_require_full_composition(self) -> None:
        self.assertEqual(
            SCOPE.classify_changed_paths(
                ["Vapor-Identity-Server", "Vapor-Registry-Server"]
            ),
            "full",
        )

    def test_root_runtime_change_requires_full_composition(self) -> None:
        self.assertEqual(
            SCOPE.classify_changed_paths(
                ["src/main.rs", "Vapor-Homepage-Server"]
            ),
            "full",
        )

    def test_unknown_root_change_defaults_to_full_composition(self) -> None:
        self.assertEqual(
            SCOPE.classify_changed_paths(["new-runtime-config.toml"]),
            "full",
        )

    def test_no_changed_paths_return_none(self) -> None:
        self.assertEqual(SCOPE.classify_changed_paths([]), "none")

    def test_push_workflow_uses_classifier_and_skips_empty_scope(self) -> None:
        workflow = (REPO_ROOT / ".github" / "workflows" / "deploy.yml").read_text(
            encoding="utf-8"
        )
        self.assertIn("paths-ignore:", workflow)
        self.assertIn("'docs/**'", workflow)
        self.assertIn("'deploy/tests/**'", workflow)
        self.assertIn("deploy/scripts/deployment_scope.py", workflow)
        self.assertIn("target != 'none'", workflow)
        self.assertIn("DISPATCH_TARGET", workflow)


if __name__ == "__main__":
    unittest.main()
