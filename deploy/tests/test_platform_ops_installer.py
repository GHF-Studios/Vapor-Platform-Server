from __future__ import annotations

import pathlib
import re
import unittest


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
INSTALLER = REPO_ROOT / "deploy" / "scripts" / "install-platform-ops.sh"
LEGACY_GROUP_RULE = (
    '%vapor-temp-ssh ALL=(root) NOPASSWD: '
    '/usr/local/sbin/vapor-ssh-deploy "", '
    '/usr/local/sbin/vapor-ssh-service-control'
)


class PlatformOpsInstallerTests(unittest.TestCase):
    def test_installer_only_removes_the_exact_legacy_group_rule(self) -> None:
        source = INSTALLER.read_text(encoding="utf-8")

        self.assertIn("legacy_sudoers=/etc/sudoers.d/vapor-temporary-access", source)
        self.assertIn(f"grep -Fqx '{LEGACY_GROUP_RULE}'", source)
        self.assertIn('rm -- "${legacy_sudoers}"', source)
        self.assertEqual(source.count(LEGACY_GROUP_RULE), 1)
        self.assertNotRegex(source, r"(?m)^\s*%vapor-temp-ssh\s+ALL\s*=")
        self.assertIn("grep -RIlE", source)
        self.assertIn("unexpected group-wide temporary-access sudo rule remains", source)
        self.assertIn("visudo -c >/dev/null", source)


if __name__ == "__main__":
    unittest.main()
