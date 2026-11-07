from __future__ import annotations

import pathlib
import unittest


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]


class RuntimeModeUnitTests(unittest.TestCase):
    def test_platform_unit_uses_configured_runtime_mode(self) -> None:
        unit = (REPO_ROOT / "deploy" / "systemd" / "vapor-platform-server.service").read_text()
        self.assertIn("EnvironmentFile=-/etc/vapor-server/platform.env", unit)
        self.assertNotIn("Environment=VAPOR_SERVER_MODE=", unit)


if __name__ == "__main__":
    unittest.main()
