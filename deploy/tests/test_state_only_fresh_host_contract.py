from __future__ import annotations

import pathlib
import unittest


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]


class StateOnlyFreshHostContractTests(unittest.TestCase):
    def test_archive_validation_and_source_pin_precede_host_package_changes(self) -> None:
        script = (REPO_ROOT / "deploy/scripts/recover-state-only-host.sh").read_text()
        checksum = 'recovery_bundle_metadata.py" --check-archive-checksum "$BUNDLE"'
        validate = 'state-bundle-validate.py" "$BUNDLE"'
        source = 'recovery_bundle_metadata.py" --source-commit "$BUNDLE"'
        packages = "apt-get update"
        self.assertIn(checksum, script)
        self.assertIn(validate, script)
        self.assertIn(source, script)
        self.assertLess(script.index(checksum), script.index(packages))
        self.assertLess(script.index(validate), script.index(packages))
        self.assertLess(script.index(source), script.index(packages))

    def test_host_must_be_empty_before_bootstrap_and_credentials_are_hidden(self) -> None:
        script = (REPO_ROOT / "deploy/scripts/recover-state-only-host.sh").read_text()
        preflight = script.index('require_empty_or_absent "$DEPLOY_ROOT"')
        prompt = script.index("Steam Web API key (hidden)")
        bootstrap = script.index("bootstrap-ubuntu.sh")
        self.assertLess(preflight, prompt)
        self.assertLess(prompt, bootstrap)
        self.assertIn("read -r -s -p 'Steam Web API key (hidden): '", script)
        self.assertIn("read -r -s -p 'GitHub OAuth client secret (hidden): '", script)
        self.assertIn("unset STEAM_KEY GITHUB_CLIENT_ID GITHUB_CLIENT_SECRET", script)

    def test_source_is_checked_out_exactly_and_application_activation_follows_state_import(self) -> None:
        script = (REPO_ROOT / "deploy/scripts/recover-state-only-host.sh").read_text()
        checkout = 'git -C "$SOURCE_ROOT" checkout --detach "$REVISION"'
        bootstrap = 'bash "$DEPLOY_ROOT/deploy/scripts/bootstrap-ubuntu.sh"'
        staged_deploy = 'bash "$DEPLOY_ROOT/deploy/scripts/deploy.sh" --approved "$REVISION"'
        restore = 'restore-state-only.sh" --bundle "$BUNDLE" --yes --fresh-host'
        self.assertIn(checkout, script)
        self.assertIn("submodule update --init --recursive", script)
        self.assertLess(script.index(checkout), script.index(bootstrap))
        self.assertLess(script.index(bootstrap), script.index(staged_deploy))
        self.assertLess(script.index(staged_deploy), script.index(restore))

    def test_fresh_host_build_defaults_to_one_job_and_validates_override(self) -> None:
        for relative_path in (
            "deploy/scripts/recover-fresh-host.sh",
            "deploy/scripts/recover-state-only-host.sh",
        ):
            script = (REPO_ROOT / relative_path).read_text()
            self.assertIn('CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-1}"', script)
            self.assertIn('[[ "${CARGO_BUILD_JOBS}" =~ ^[1-9][0-9]*$ ]]', script)

    def test_usb_sync_stores_a_source_pinned_recovery_kit_next_to_state_snapshots(self) -> None:
        script = (REPO_ROOT / "deploy/scripts/backup-state-only-to-usb.ps1").read_text()
        self.assertIn("Recovery-Kit\\state-only", script)
        self.assertIn(
            '& $git -C $repositoryRoot archive --format=zip --output=$kitZip $sourceCommit @archivePaths',
            script,
        )
        self.assertIn("archive_sha256", script)


if __name__ == "__main__":
    unittest.main()
