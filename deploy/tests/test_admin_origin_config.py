from __future__ import annotations

import pathlib
import subprocess
import sys
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[2]
RENDERER = ROOT / "deploy" / "scripts" / "render-caddyfile.py"
PUBLIC = "vapor.example.test"
ADMIN = "admin.vapor.example.test"


class AdminOriginConfigTests(unittest.TestCase):
    def render(self, template_name: str, admin_domain: str | None) -> str:
        template = ROOT / "deploy" / "caddy" / template_name
        args = [sys.executable, str(RENDERER), "--template", str(template), "--public-domain", PUBLIC]
        if admin_domain is not None:
            args.extend(["--admin-domain", admin_domain])
        result = subprocess.run(args, capture_output=True, text=True, check=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout

    def test_configured_admin_host_receives_dashboard_and_identity_routes(self) -> None:
        rendered = self.render("Caddyfile.template", ADMIN)
        self.assertIn(f"{ADMIN} {{", rendered)
        admin_routes = rendered.split("(vapor_admin_routes)", 1)[1].split("{{", 1)[0]
        self.assertIn("/admin*", admin_routes)
        self.assertIn("/api/admin*", admin_routes)
        self.assertIn("/api/identity/*", rendered.split(f"{ADMIN} {{", 1)[0])
        self.assertIn("/emergency-terminal/*", admin_routes)

    def test_configured_admin_host_removes_privileged_public_routes(self) -> None:
        rendered = self.render("Caddyfile.template", ADMIN)
        public_routes = rendered.split("(vapor_routes) {", 1)[1].split("(vapor_admin_routes)", 1)[0]
        identity = "127.0.0.1:7113"
        self.assertIn(f"redir https://{ADMIN}{{http.request.uri}} 308", public_routes)
        self.assertIn(f"handle /login* {{\n\t\treverse_proxy {identity}", public_routes)
        self.assertIn(f"handle /logout {{\n\t\treverse_proxy {identity}", public_routes)
        self.assertIn(f"handle /operator/* {{\n\t\tredir https://{ADMIN}{{http.request.uri}} 308", public_routes)
        self.assertIn('handle /api/admin* {\n\t\trespond "not found" 404', public_routes)
        self.assertIn('handle /api/identity/v1/admin* {\n\t\trespond "not found" 404', public_routes)
        self.assertIn('handle /api/identity/v1/internal/* {\n\t\trespond "not found" 404', public_routes)
        self.assertIn('handle /emergency-terminal/* {\n\t\trespond "not found" 404', public_routes)
        self.assertNotIn("handle /admin* {\n\t\treverse_proxy", public_routes)
        for path in ("/admin*", "/operator/*", "/cli/ssh/*"):
            self.assertIn(f"handle {path} {{\n\t\tredir", public_routes)
        self.assertIn(f"handle_path /api/identity/* {{\n\t\treverse_proxy {identity}", public_routes)

    def test_admin_host_has_identity_and_platform_routes(self) -> None:
        rendered = self.render("Caddyfile.template", ADMIN)
        admin_site = rendered.split("(vapor_admin_routes) {", 1)[1].split("\n}\n", 1)[0]
        for route in ("/admin*", "/api/admin*", "/operator/*", "/cli/ssh/*"):
            self.assertIn(f"handle {route} {{", admin_site)
        self.assertNotIn("handle /login* {", admin_site)
        self.assertNotIn("handle /logout {", admin_site)
        self.assertIn("/api/identity/*", admin_site)
        identity_upstream = "127.0.0.1:7113"
        self.assertIn(f"reverse_proxy {identity_upstream}", admin_site)
        self.assertIn('handle /api/identity/v1/internal/* {\n\t\trespond "not found" 404', admin_site)
        self.assertIn('redir /admin 302', admin_site)

    def test_unconfigured_admin_host_preserves_existing_routes_without_admin_site(self) -> None:
        rendered = self.render("Caddyfile.template", None)
        self.assertNotIn("admin.vapor.example.test {", rendered)
        self.assertIn("handle /admin* {\n\t\treverse_proxy 127.0.0.1:7110", rendered)
        self.assertIn("handle /login* {\n\t\treverse_proxy 127.0.0.1:7113", rendered)
        self.assertIn("handle /operator/* {\n\t\treverse_proxy 127.0.0.1:7113", rendered)

    def test_admin_host_example_keeps_public_account_login_and_isolates_operator_login(self) -> None:
        example = (ROOT / "deploy" / "caddy" / "Caddyfile.admin-host.example").read_text(encoding="utf-8")
        public_routes = example.split("(vapor_routes) {", 1)[1].split("(vapor_admin_routes)", 1)[0]
        admin_routes = example.split("(vapor_admin_routes) {", 1)[1].split("\n}\n", 1)[0]
        self.assertIn("handle /login* {\n\t\treverse_proxy 127.0.0.1:7113", public_routes)
        self.assertIn("handle /operator/* {\n\t\tredir https://admin.example.test{http.request.uri} 308", public_routes)
        self.assertIn("handle /operator/* {\n\t\treverse_proxy 127.0.0.1:7113", admin_routes)
        self.assertNotIn("handle /login* {", admin_routes)

    def test_admin_domain_must_be_a_distinct_dns_hostname(self) -> None:
        for invalid in (PUBLIC, "admin.example.test\nrespond 200", "https://admin.example.test", "127.0.0.1"):
            with self.subTest(invalid=invalid):
                template = ROOT / "deploy" / "caddy" / "Caddyfile.template"
                result = subprocess.run(
                    [sys.executable, str(RENDERER), "--template", str(template), "--public-domain", PUBLIC, "--admin-domain", invalid],
                    capture_output=True,
                    text=True,
                    check=False,
                )
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")


if __name__ == "__main__":
    unittest.main()
