from __future__ import annotations

import pathlib
import unittest


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]


class RecoveryActivationOrderTests(unittest.TestCase):
    def test_full_bundle_validation_precedes_package_changes(self) -> None:
        coordinator = (REPO_ROOT / "deploy/scripts/recover-fresh-host.sh").read_text()
        validate = 'python3 "${SCRIPT_DIR}/state-bundle-validate.py" "${BUNDLE}"'
        self.assertIn(validate, coordinator)
        self.assertLess(coordinator.index(validate), coordinator.index("apt-get update"))
        checksums = 'sha256sum --check --status SHA256SUMS'
        source_pin = 'recovery_bundle_metadata.py" --source-commit "${BUNDLE}"'
        self.assertIn(checksums, coordinator)
        self.assertLess(coordinator.index(checksums), coordinator.index(source_pin))
        self.assertLess(coordinator.index(source_pin), coordinator.index(validate))
        self.assertLess(coordinator.index(validate), coordinator.index("apt-get update"))
        self.assertIn("preflight requires the Ubuntu-provided Python 3.11+ runtime", coordinator)
        host_check = '--check-host "${BUNDLE}"'
        self.assertIn(host_check, coordinator)
        self.assertLess(coordinator.index(host_check), coordinator.index("apt-get update"))

        export = (REPO_ROOT / "deploy/scripts/export-state.sh").read_text()
        self.assertIn("state-bundle-validate.py", export)
        self.assertIn("deployed_source_manifest.py", export)

    def test_fresh_host_stages_before_restoring_and_only_restore_activates(self) -> None:
        coordinator = (REPO_ROOT / "deploy/scripts/recover-fresh-host.sh").read_text()
        deploy = (REPO_ROOT / "deploy/scripts/deploy.sh").read_text()
        stage_at = coordinator.index("VAPOR_DEPLOY_BOOTSTRAP=1 VAPOR_RECOVERY_STAGING=1")
        restore_call = 'bash "${DEPLOY_ROOT}/deploy/scripts/restore-state.sh" --bundle "${BUNDLE}" --yes'
        self.assertIn(restore_call, coordinator)
        restore_at = coordinator.index(restore_call)
        self.assertLess(stage_at, restore_at)
        self.assertIn('bash "${DEPLOY_ROOT}/deploy/scripts/deploy.sh" --approved "${REVISION}"', coordinator[stage_at:restore_at])
        self.assertIn('VAPOR_RECOVERY_STAGING', deploy)
        self.assertIn('recovery runtime staged; restore-state.sh owns activation', deploy)
        staged = deploy.index('recovery runtime staged; restore-state.sh owns activation')
        self.assertLess(staged, deploy.index('systemctl restart vapor-platform-server.service'))
        self.assertLess(staged, deploy.index('"${VAPOR_DEPLOY_ROOT}/deploy/scripts/health-check.sh"'))

    def test_recovery_staging_does_not_enable_units_or_start_package_services(self) -> None:
        bootstrap = (REPO_ROOT / "deploy/scripts/bootstrap-ubuntu.sh").read_text()
        self.assertIn("policy-rc.d", bootstrap)
        self.assertIn("exit 101", bootstrap)
        self.assertIn("mktemp /usr/sbin/.policy-rc.d.recovery.XXXXXX", bootstrap)
        self.assertIn('ln -- "${recovery_policy_tmp}" "${recovery_policy}"', bootstrap)
        self.assertLess(bootstrap.index("policy-rc.d"), bootstrap.index("apt-get install --yes caddy"))
        self.assertIn("systemctl disable --now caddy.service", bootstrap)

        for relative, activation in [
            ("deploy/scripts/install-systemd.sh", "systemctl enable vapor-platform-server.service"),
            ("deploy/scripts/install-services-systemd.sh", 'systemctl enable "vapor-${service}.service"'),
            ("deploy/scripts/install-caddy.sh", "systemctl enable caddy.service"),
            ("deploy/scripts/install-state-backup.sh", "systemctl enable --now vapor-state-export.timer"),
        ]:
            with self.subTest(path=relative):
                text = (REPO_ROOT / relative).read_text()
                staged = text.index('"${VAPOR_RECOVERY_STAGING:-0}"')
                self.assertLess(staged, text.index(activation))


if __name__ == "__main__":
    unittest.main()
