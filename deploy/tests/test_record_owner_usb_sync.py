import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).parents[1] / "scripts" / "record_owner_usb_sync.py"
SPEC = importlib.util.spec_from_file_location("record_owner_usb_sync", SCRIPT)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(MODULE)


class OwnerUsbSyncRecorderTests(unittest.TestCase):
    def valid_record(self):
        return {
            "schema_version": 1,
            "last_success_unix": 1794076800,
            "latest_archive": "vapor-platform-recovery-20261107T120000Z-0123456789ab.tar.gz",
            "latest_archive_sha256": "a" * 64,
            "verified_archive_count": 12,
            "unverified_archive_count": 0,
            "retained_archive_payload_bytes": 1024,
            "latest_rehashed": True,
            "recovery_kit_available": True,
            "recovery_kit_preflight_validation": True,
            "recovery_kit_source_commit": "0123456789abcdef0123456789abcdef01234567",
        }

    def test_valid_report_is_allowlisted_and_atomically_recorded(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "backup"
            root.mkdir()
            encoded = MODULE.encode_record(self.valid_record())
            stored = MODULE.record_encoded(encoded, root, now_unix=1794076810)
            self.assertEqual(stored["latest_archive"], self.valid_record()["latest_archive"])
            self.assertTrue((root / "owner-usb-sync.json").is_file())
            self.assertFalse(list(root.glob("*.partial")))

    def test_unknown_fields_are_rejected_without_replacing_previous_record(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "backup"
            root.mkdir()
            old = json.dumps(self.valid_record()).encode()
            (root / "owner-usb-sync.json").write_bytes(old)
            invalid = self.valid_record() | {"drive_letter": "E:"}
            with self.assertRaises(ValueError):
                MODULE.record_encoded(MODULE.encode_record(invalid), root, now_unix=1794076810)
            self.assertEqual((root / "owner-usb-sync.json").read_bytes(), old)

    def test_rejects_malformed_commit_and_unsafe_archive_digest(self):
        record = self.valid_record()
        record["recovery_kit_source_commit"] = "not-a-commit"
        with self.assertRaises(ValueError):
            MODULE.validate_record(record, now_unix=1794076810)
        record = self.valid_record()
        record["latest_archive_sha256"] = "../" + "a" * 61
        with self.assertRaises(ValueError):
            MODULE.validate_record(record, now_unix=1794076810)

    def test_preserves_unverified_count_as_a_warning_metric(self):
        record = self.valid_record()
        record["unverified_archive_count"] = 3
        checked = MODULE.validate_record(record, now_unix=1794076810)
        self.assertEqual(checked["unverified_archive_count"], 3)


if __name__ == "__main__":
    unittest.main()
