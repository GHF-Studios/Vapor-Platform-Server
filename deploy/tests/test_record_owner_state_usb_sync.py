from __future__ import annotations

import base64
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).parents[1] / "scripts" / "record_owner_state_usb_sync.py"
SPEC = importlib.util.spec_from_file_location("record_owner_state_usb_sync", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class RecordOwnerStateUsbSyncTests(unittest.TestCase):
    def report(self) -> dict[str, object]:
        return {
            "schema_version": 1,
            "last_success_unix": 1_800_000_000,
            "latest_archive": "vapor-platform-state-20261108T120000Z-012345abcdef.tar.gz",
            "latest_archive_sha256": "a" * 64,
            "verified_generation_count": 12,
            "retained_payload_bytes": 4096,
            "source_commit": "0123456789abcdef0123456789abcdef01234567",
            "latest_rehashed": True,
            "recovery_kit_available": True,
        }

    def test_records_only_allowlisted_owner_verified_state(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            encoded = base64.b64encode(json.dumps(self.report()).encode()).decode()
            MODULE.record_encoded(encoded, root, now_unix=1_800_000_100)
            saved = json.loads((root / "owner-usb-sync.json").read_text())
            self.assertEqual(saved, self.report())
            if MODULE.os.name == "posix":
                self.assertEqual((root / "owner-usb-sync.json").stat().st_mode & 0o777, 0o600)

    def test_rejects_invalid_archive_and_arbitrary_fields_without_overwriting(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            destination = root / "owner-usb-sync.json"
            original = b'{"known":"previous"}\n'
            destination.write_bytes(original)
            bad = self.report()
            bad["latest_archive"] = "../../other.tar.gz"
            encoded = base64.b64encode(json.dumps(bad).encode()).decode()
            with self.assertRaises(ValueError):
                MODULE.record_encoded(encoded, root, now_unix=1_800_000_100)
            extra = self.report()
            extra["extra"] = "not accepted"
            encoded = base64.b64encode(json.dumps(extra).encode()).decode()
            with self.assertRaises(ValueError):
                MODULE.record_encoded(encoded, root, now_unix=1_800_000_100)
            self.assertEqual(destination.read_bytes(), original)


if __name__ == "__main__":
    unittest.main()
