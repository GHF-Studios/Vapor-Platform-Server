from __future__ import annotations

import pathlib
import unittest


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]


class StateOnlyRestoreContractTests(unittest.TestCase):
    def test_state_restore_validates_archive_before_mutation_and_never_touches_host_config(self) -> None:
        restore = (REPO_ROOT / "deploy/scripts/restore-state-only.sh").read_text()
        checksum = 'recovery_bundle_metadata.py" --check-archive-checksum "${BUNDLE}"'
        validate = 'state-bundle-validate.py" "${BUNDLE}"'
        lock = 'exec 9>/run/vapor-server-deploy.lock'
        mutation = 'MUTATION_STARTED=true'
        self.assertIn(checksum, restore)
        self.assertIn(validate, restore)
        self.assertLess(restore.index(checksum), restore.index(validate))
        self.assertLess(restore.index(validate), restore.index(lock))
        self.assertLess(restore.index(lock), restore.index(mutation))
        for forbidden in ("/etc/vapor-server", "/etc/caddy", "/etc/systemd/system", "git checkout"):
            self.assertNotIn(forbidden, restore)

    def test_state_restore_quiesces_service_writers_before_preserving_or_replacing_data(self) -> None:
        restore = (REPO_ROOT / "deploy/scripts/restore-state-only.sh").read_text()
        stop_services = restore.index('for service in "${SERVICES[@]}"; do\n  if systemctl is-active')
        pause_expiry = restore.index("pause_temporary_ssh_expiry\n\nMUTATION_STARTED=true")
        preserve_live = restore.index('mv -- "${target}" "${PRE_RESTORE}/state/${relative}"')
        replace_live = restore.index('cp -a -- "${source}" "${target}"')
        self.assertLess(stop_services, pause_expiry)
        self.assertLess(pause_expiry, preserve_live)
        self.assertLess(preserve_live, replace_live)
        self.assertIn('"${SCRIPT_DIR}/health-check.sh"', restore)
        self.assertIn('pre-state-only-restore-', restore)

    def test_state_restore_requires_a_configured_live_host_before_taking_the_lock(self) -> None:
        restore = (REPO_ROOT / "deploy/scripts/restore-state-only.sh").read_text()
        config_check = '"${VAPOR_CONFIG_DIR}/platform.env"'
        platform_check = 'systemctl is-active --quiet vapor-platform-server.service'
        gateway_check = 'systemctl is-active --quiet caddy.service'
        lock = 'exec 9>/run/vapor-server-deploy.lock'
        self.assertIn(config_check, restore)
        self.assertIn(platform_check, restore)
        self.assertIn(gateway_check, restore)
        self.assertLess(restore.index(config_check), restore.index(lock))
        self.assertLess(restore.index(platform_check), restore.index(lock))
        self.assertLess(restore.index(gateway_check), restore.index(lock))

    def test_fresh_host_restore_reconciles_old_ssh_grants_and_activates_only_after_state_copy(self) -> None:
        restore = (REPO_ROOT / "deploy/scripts/restore-state-only.sh").read_text()
        reconcile = 'reconcile-fresh-host-ssh.py" "${BUNDLE_STATE}/platform/platform.sqlite3'
        copy_state = 'cp -a -- "${source}" "${target}"'
        activate = 'systemctl enable --now vapor-platform-server.service'
        health = '"${SCRIPT_DIR}/health-check.sh"'
        self.assertIn("--fresh-host", restore)
        self.assertIn(reconcile, restore)
        self.assertLess(restore.index(reconcile), restore.index(copy_state))
        self.assertLess(restore.index(copy_state), restore.index(activate))
        self.assertLess(restore.index(activate), restore.index(health))
        self.assertIn("services to remain staged and inactive", restore)

    def test_state_only_export_is_separate_from_legacy_archive_storage(self) -> None:
        export = (REPO_ROOT / "deploy/scripts/export-state-only.sh").read_text()
        self.assertIn("/state-only", export)
        self.assertIn("vapor-platform-state-", export)
        self.assertIn("state_catalog_version", export)
        self.assertIn("platform_source_commit", export)
        self.assertIn("rev-parse --verify 'HEAD^{commit}'", export)
        self.assertNotIn('cp -a "${VAPOR_CONFIG_DIR}', export)
        self.assertNotIn("/etc/caddy", export)
        self.assertNotIn("recovery-kit", export)
        self.assertIn('snapshot-state-databases.py" "${VAPOR_STATE_ROOT}"', export)
        self.assertNotIn("systemctl stop", export)
        self.assertNotIn("pause_temporary_ssh_expiry", export)


if __name__ == "__main__":
    unittest.main()
