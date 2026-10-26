from __future__ import annotations

import os
import pathlib
import subprocess
import tempfile
import unittest


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
DEPLOYER = REPO_ROOT / "deploy" / "scripts" / "deploy-atlas.sh"


class AtlasPublicationTests(unittest.TestCase):
    def test_deployment_builds_switches_retains_and_preserves_current_on_failure(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            fake_bin = root / "bin"
            fake_bin.mkdir()
            deploy_root = root / "platform"
            atlas_repo = deploy_root / "Atlas"
            (atlas_repo / ".git").mkdir(parents=True)
            web_root = root / "web" / "atlas"
            calls = root / "python-calls"

            (fake_bin / "id").write_text("#!/bin/sh\necho 0\n")
            (fake_bin / "install").write_text(
                "#!/bin/sh\n"
                "for path do last=\"$path\"; done\n"
                "mkdir -p \"$last\"\n"
            )
            (fake_bin / "git").write_text(
                "#!/bin/sh\n"
                "if [ \"$1\" = check-ref-format ]; then exit 0; fi\n"
                "case \"$*\" in\n"
                "  *fetch*) exit 0 ;;\n"
                "  *rev-parse*) printf '%s\\n' \"$ATLAS_TEST_REVISION\" ;;\n"
                "  *checkout*) exit 0 ;;\n"
                "  *) echo \"unexpected git invocation: $*\" >&2; exit 1 ;;\n"
                "esac\n"
            )
            (fake_bin / "python3").write_text(
                "#!/bin/sh\n"
                "printf '%s\\n' \"$*\" >> \"$ATLAS_TEST_CALLS\"\n"
                "case \"$2\" in\n"
                "  validate) [ -z \"${ATLAS_TEST_FAIL:-}\" ] ;;\n"
                "  build)\n"
                "    [ -z \"${ATLAS_TEST_FAIL:-}\" ] || exit 1\n"
                "    while [ $# -gt 0 ]; do if [ \"$1\" = --output ]; then shift; output=$1; fi; shift; done\n"
                "    mkdir -p \"$output\"\n"
                "    printf '%s' \"$ATLAS_TEST_REVISION\" > \"$output/index.html\"\n"
                "    printf '{}' > \"$output/atlas.json\"\n"
                "    ;;\n"
                "  *) echo \"unexpected Atlas command: $*\" >&2; exit 1 ;;\n"
                "esac\n"
            )
            for executable in fake_bin.iterdir():
                executable.chmod(0o755)

            environment = os.environ.copy()
            environment.update(
                {
                    "PATH": f"{fake_bin}:{environment['PATH']}",
                    "VAPOR_DEPLOY_ROOT": str(deploy_root),
                    "VAPOR_ATLAS_WEB_ROOT": str(web_root),
                    "VAPOR_CONFIG_DIR": str(root / "no-config"),
                    "ATLAS_TEST_CALLS": str(calls),
                    "ATLAS_TEST_REVISION": "a" * 40,
                }
            )

            def deploy(revision: str, *, fail: bool = False) -> subprocess.CompletedProcess[str]:
                environment["ATLAS_TEST_REVISION"] = revision
                if fail:
                    environment["ATLAS_TEST_FAIL"] = "1"
                else:
                    environment.pop("ATLAS_TEST_FAIL", None)
                return subprocess.run(
                    [str(DEPLOYER)],
                    check=False,
                    env=environment,
                    capture_output=True,
                    text=True,
                )

            first = "a" * 40
            second = "b" * 40
            third = "c" * 40
            failed = "d" * 40

            result = deploy(first)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(os.readlink(web_root / "current"), f"releases/{first}")
            self.assertEqual((web_root / "current" / "index.html").read_text(), first)

            result = deploy(first)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("already published", result.stdout)
            self.assertEqual(len(calls.read_text().splitlines()), 2)

            result = deploy(second)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(os.readlink(web_root / "current"), f"releases/{second}")
            self.assertEqual(os.readlink(web_root / "previous"), f"releases/{first}")

            result = deploy(third)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(os.readlink(web_root / "current"), f"releases/{third}")
            self.assertEqual(os.readlink(web_root / "previous"), f"releases/{second}")
            self.assertFalse((web_root / "releases" / first).exists())

            result = deploy(failed, fail=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(os.readlink(web_root / "current"), f"releases/{third}")
            self.assertEqual(os.readlink(web_root / "previous"), f"releases/{second}")
            self.assertFalse(any(web_root.glob(".staging.*")))

    def test_caddy_serves_atlas_from_its_independent_static_root(self) -> None:
        caddy = (REPO_ROOT / "deploy" / "caddy" / "Caddyfile.template").read_text()
        self.assertIn("handle_path /atlas/*", caddy)
        self.assertIn("root * /srv/vapor-web/atlas/current", caddy)
        self.assertIn("file_server", caddy)
        self.assertNotIn("127.0.0.1:7112", caddy.split("handle_path /atlas/*", 1)[1].split("handle_path /docs/*", 1)[0])


if __name__ == "__main__":
    unittest.main()
