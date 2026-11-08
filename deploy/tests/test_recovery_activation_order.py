from __future__ import annotations

import pathlib
import unittest


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]


class RecoveryActivationOrderTests(unittest.TestCase):
    def test_export_quiesces_temporary_ssh_expiry_work_around_state_copy(self) -> None:
        export = (REPO_ROOT / "deploy/scripts/export-state.sh").read_text()
        self.assertIn("pause_temporary_ssh_expiry", export)
        self.assertIn("restart_temporary_ssh_expiry", export)
        stop_services_at = export.index('for service in "${STATEFUL_SERVICES[@]}"; do')
        pause_at = export.index("\npause_temporary_ssh_expiry\n", stop_services_at)
        copy_state_at = export.index('tar \\\n    --exclude=\'./build-cache/targets\'')
        restart_timers_at = export.index("restart_temporary_ssh_expiry", copy_state_at)
        archive_at = export.index('tar -C "${TMP_DIR}" -czf')
        self.assertLess(stop_services_at, pause_at)
        self.assertLess(stop_services_at, copy_state_at)
        self.assertLess(copy_state_at, restart_timers_at)
        self.assertLess(restart_timers_at, archive_at)
        quiescence = (REPO_ROOT / "deploy/scripts/ssh-expiry-quiescence.sh").read_text()
        self.assertIn("ssh-expiry-quiescence.sh", export)
        self.assertIn("vapor-ssh-expire-*.timer", quiescence)
        self.assertIn("vapor-ssh-expire-*.service", quiescence)

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
        self.assertIn("service_deploy_map.py", export)

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
        self.assertIn('recovery runtime staged; restore-state-only.sh owns activation', deploy)
        staged = deploy.index('recovery runtime staged; restore-state-only.sh owns activation')
        self.assertLess(staged, deploy.index('systemctl restart vapor-platform-server.service'))
        self.assertLess(staged, deploy.index('"${VAPOR_DEPLOY_ROOT}/deploy/scripts/health-check.sh"'))
        self.assertIn('restore-state.sh" --bundle "${BUNDLE}" --yes --fresh-host', coordinator)

    def test_fresh_host_restore_does_not_install_expiry_timers_for_discarded_ssh_grants(self) -> None:
        restore = (REPO_ROOT / "deploy/scripts/restore-state.sh").read_text()
        self.assertIn('--fresh-host', restore)
        self.assertIn('reconcile-fresh-host-ssh.py', restore)
        self.assertIn('vapor-ssh-expire-*.timer', restore)
        self.assertIn('vapor-ssh-expire-*.service', restore)

    def test_restore_binds_checksum_sidecar_to_bundle_before_archive_validation(self) -> None:
        restore = (REPO_ROOT / "deploy/scripts/restore-state.sh").read_text()
        checksum = 'recovery_bundle_metadata.py" --check-archive-checksum "${BUNDLE_PATH}"'
        validate = 'state-bundle-validate.py" "${BUNDLE_PATH}"'
        self.assertIn(checksum, restore)
        self.assertIn(validate, restore)
        self.assertLess(restore.index(checksum), restore.index(validate))
        self.assertLess(restore.index(validate), restore.index('MODE="$(python3 -'))

    def test_restore_quiesces_old_expiry_jobs_before_swapping_state(self) -> None:
        restore = (REPO_ROOT / "deploy/scripts/restore-state.sh").read_text()
        mutation_at = restore.index('MUTATION_STARTED="true"')
        stop_services_at = restore.index('for service in "${STATEFUL_SERVICES[@]}"; do stop_if_loaded')
        pause_at = restore.index("pause_temporary_ssh_expiry", stop_services_at)
        move_state_at = restore.index('mv -- "${VAPOR_STATE_ROOT}" "${PRE_RESTORE_PATH}/state"')
        self.assertLess(mutation_at, stop_services_at)
        self.assertLess(stop_services_at, pause_at)
        self.assertLess(pause_at, move_state_at)
        self.assertIn("ssh-expiry-quiescence.sh", restore)
        self.assertIn("restored_ssh_expiry_units.py", restore)
        self.assertLess(restore.index('activate_restored_ssh_expiry\nfi'), restore.index('systemctl enable --now vapor-state-only-export.timer'))

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
            ("deploy/scripts/install-state-backup.sh", "systemctl enable --now vapor-state-only-export.timer"),
        ]:
            with self.subTest(path=relative):
                text = (REPO_ROOT / relative).read_text()
                staged = text.index('"${VAPOR_RECOVERY_STAGING:-0}"')
                self.assertLess(staged, text.index(activation))


if __name__ == "__main__":
    unittest.main()
