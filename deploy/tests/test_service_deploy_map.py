from __future__ import annotations

import importlib.util
import pathlib
import unittest


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
MAP_PATH = REPO_ROOT / "deploy" / "scripts" / "service_deploy_map.py"
SPEC = importlib.util.spec_from_file_location("service_deploy_map", MAP_PATH)
DEPLOY_MAP = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(DEPLOY_MAP)


class ServiceDeployMapTests(unittest.TestCase):
    def test_each_target_has_only_fixed_deployer_metadata(self) -> None:
        caddy = (REPO_ROOT / "deploy" / "caddy" / "Caddyfile.services.template").read_text(encoding="utf-8")
        for target, record in DEPLOY_MAP.DEPLOY_TARGETS.items():
            with self.subTest(target=target):
                self.assertIn(record["kind"], {"service", "static"})
                self.assertTrue(record["package_path"])
                self.assertTrue(record["public_health"].startswith("/"))
                if target == "atlas":
                    self.assertEqual(record["kind"], "static")
                    self.assertIn("handle_path /atlas/*", caddy)
                    self.assertEqual(record["public_health"], "/atlas/")
                    continue
                self.assertTrue(record["binary"])
                self.assertTrue(record["unit"].endswith(".service"))
                self.assertTrue(record["loopback_health"].startswith("http://127.0.0.1:"))
                self.assertTrue(record["state_name"])
                unit = (REPO_ROOT / "deploy" / "systemd" / record["unit"]).read_text(encoding="utf-8")
                self.assertIn(record["binary"], unit)
                if target in {"docs", "identity", "diagnostics", "registry", "build-cache"}:
                    prefix = {
                        "docs": "/docs/",
                        "identity": "/api/identity/",
                        "diagnostics": "/api/diagnostics/",
                        "registry": "/api/registry/",
                        "build-cache": "/api/build-cache/",
                    }[target]
                    self.assertIn(f"handle_path {prefix}*", caddy)
                    self.assertEqual(record["public_health"], prefix.rstrip("/") + "/healthz")
                if target == "artifact":
                    self.assertIn("handle /api/updates/*", caddy)
                    self.assertEqual(record["public_health"], "/api/updates/healthz")

    def test_invalid_target_cannot_be_resolved(self) -> None:
        with self.assertRaisesRegex(ValueError, "not allowlisted"):
            DEPLOY_MAP.target_map("../../etc/passwd")

    def test_immutable_artifact_stores_are_not_copied_during_binary_only_deployments(self) -> None:
        for target in {"artifact", "build-cache"}:
            self.assertEqual(DEPLOY_MAP.target_map(target)["state_backup"], "preserve-in-place")
        for target in set(DEPLOY_MAP.DEPLOY_TARGETS) - {"atlas", "artifact", "build-cache"}:
            with self.subTest(target=target):
                self.assertEqual(DEPLOY_MAP.target_map(target)["state_backup"], "snapshot")


if __name__ == "__main__":
    unittest.main()
