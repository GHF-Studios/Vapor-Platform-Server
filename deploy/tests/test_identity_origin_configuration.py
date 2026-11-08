from __future__ import annotations

import pathlib
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[2]


class IdentityOriginConfigurationTests(unittest.TestCase):
    def test_fresh_bootstrap_persists_owner_entered_provider_credentials_without_echoing_them(self) -> None:
        bootstrap = (ROOT / "deploy" / "scripts" / "bootstrap-ubuntu.sh").read_text(encoding="utf-8")
        self.assertIn("VAPOR_IDENTITY_STEAM_WEB_API_KEY=${VAPOR_IDENTITY_STEAM_WEB_API_KEY}", bootstrap)
        self.assertIn("VAPOR_IDENTITY_GITHUB_CLIENT_ID=${VAPOR_IDENTITY_GITHUB_CLIENT_ID}", bootstrap)
        self.assertIn("VAPOR_IDENTITY_GITHUB_CLIENT_SECRET=${VAPOR_IDENTITY_GITHUB_CLIENT_SECRET}", bootstrap)
        self.assertIn("read -r -s -p 'Steam Web API key (hidden): '", (ROOT / "deploy" / "scripts" / "recover-state-only-host.sh").read_text(encoding="utf-8"))

    def test_bootstrap_keeps_user_and_operator_origins_distinct(self) -> None:
        bootstrap = (ROOT / "deploy" / "scripts" / "bootstrap-ubuntu.sh").read_text(encoding="utf-8")
        self.assertIn("VAPOR_IDENTITY_PUBLIC_ORIGIN=https://${VAPOR_DOMAIN}", bootstrap)
        self.assertIn("VAPOR_IDENTITY_OPERATOR_ORIGIN=${VAPOR_PLATFORM_ADMIN_ORIGIN}", bootstrap)
        self.assertNotIn("VAPOR_IDENTITY_PUBLIC_ORIGIN=${VAPOR_PLATFORM_ADMIN_ORIGIN}", bootstrap)

    def test_deploy_reconciles_both_identity_origins_from_platform_configuration(self) -> None:
        deploy = (ROOT / "deploy" / "scripts" / "deploy.sh").read_text(encoding="utf-8")
        library = (ROOT / "deploy" / "scripts" / "lib.sh").read_text(encoding="utf-8")
        self.assertIn('--public-origin "${VAPOR_PLATFORM_PUBLIC_ORIGIN}"', deploy)
        self.assertIn('--operator-origin "${VAPOR_PLATFORM_ADMIN_ORIGIN}"', deploy)
        self.assertIn('VAPOR_PLATFORM_PUBLIC_ORIGIN="${VAPOR_PLATFORM_PUBLIC_ORIGIN:-https://${VAPOR_DOMAIN}}"', library)

    def test_health_check_keeps_public_login_on_public_host(self) -> None:
        health = (ROOT / "deploy" / "scripts" / "health-check.sh").read_text(encoding="utf-8")
        self.assertIn('"https://${VAPOR_DOMAIN}/login"', health)
        self.assertIn("expected 200", health)
        self.assertIn('"https://${VAPOR_ADMIN_DOMAIN}/operator/login"', health)
        self.assertIn("for path in /operator/login /cli/ssh/authorize", health)
        self.assertNotIn("for path in /login /logout /cli/ssh/authorize", health)


if __name__ == "__main__":
    unittest.main()
