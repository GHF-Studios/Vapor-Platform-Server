from __future__ import annotations

import importlib.util
import os
import pathlib
import tempfile
import unittest


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
MODULE_PATH = REPO_ROOT / "deploy" / "scripts" / "restore_host_preflight.py"
SPEC = importlib.util.spec_from_file_location("restore_host_preflight", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class RestoreHostPreflightTests(unittest.TestCase):
    def test_accepts_regular_managed_units_and_caddyfile(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            systemd = root / "systemd"
            systemd.mkdir()
            (systemd / "vapor-platform-server.service").write_text("[Service]\n")
            caddy = root / "Caddyfile"
            caddy.write_text(":80 {}\n")
            MODULE.validate_host_state(systemd, caddy)

    def test_rejects_directory_with_managed_unit_name(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            systemd = root / "systemd"
            systemd.mkdir()
            (systemd / "vapor-platform-server.service").mkdir()
            with self.assertRaises(MODULE.HostStateError):
                MODULE.validate_host_state(systemd, root / "Caddyfile")

    @unittest.skipIf(os.name == "nt", "creating native symlinks requires Windows symlink privileges")
    def test_rejects_masked_unit_symlink(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            systemd = root / "systemd"
            systemd.mkdir()
            (systemd / "vapor-platform-server.service").symlink_to("/dev/null")
            with self.assertRaises(MODULE.HostStateError):
                MODULE.validate_host_state(systemd, root / "Caddyfile")

    @unittest.skipIf(os.name == "nt", "creating native symlinks requires Windows symlink privileges")
    def test_rejects_caddyfile_symlink(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            systemd = root / "systemd"
            systemd.mkdir()
            caddy = root / "Caddyfile"
            caddy.write_text(":80 {}\n")
            link = root / "Caddyfile-link"
            link.symlink_to(caddy)
            with self.assertRaises(MODULE.HostStateError):
                MODULE.validate_host_state(systemd, link)


if __name__ == "__main__":
    unittest.main()
