from __future__ import annotations

import importlib.util
import hashlib
import json
import pathlib
import tempfile
import unittest
from unittest import mock


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
MODULE_PATH = REPO_ROOT / "deploy" / "scripts" / "plan_recovery_retention.py"
SPEC = importlib.util.spec_from_file_location("plan_recovery_retention", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class RecoveryRetentionTests(unittest.TestCase):
    def add_archive(self, root: pathlib.Path, name: str, _digest: str) -> dict[str, object]:
        archive = root / name
        contents = name.encode("ascii")
        archive.write_bytes(contents)
        digest = hashlib.sha256(contents).hexdigest()
        (root / f"{name}.sha256").write_text(f"{digest}  {name}\n", encoding="ascii")
        return {"archive": name, "sha256": digest, "size_bytes": archive.stat().st_size}

    def test_keeps_new_and_current_latest_and_prunes_oldest(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            first = self.add_archive(root, "vapor-platform-recovery-20261101T000000Z-aaaaaaaaaaaa.tar.gz", "a" * 64)
            current = self.add_archive(root, "vapor-platform-recovery-20261102T000000Z-bbbbbbbbbbbb.tar.gz", "b" * 64)
            incoming = self.add_archive(root, "vapor-platform-recovery-20261103T000000Z-cccccccccccc.tar.gz", "c" * 64)
            (root / "index.json").write_text(json.dumps({"schema_version": 1, "archives": [first, current]}))
            (root / "latest.json").write_text(json.dumps({"schema_version": 1, **current}))

            plan = MODULE.plan_prune_names(root, str(incoming["archive"]), 2)

            self.assertEqual(plan, (str(first["archive"]),))

    def test_skips_pruning_when_current_pointer_is_not_trustworthy(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            old = self.add_archive(root, "vapor-platform-recovery-20261101T000000Z-aaaaaaaaaaaa.tar.gz", "a" * 64)
            new = self.add_archive(root, "vapor-platform-recovery-20261102T000000Z-bbbbbbbbbbbb.tar.gz", "b" * 64)
            (root / "index.json").write_text(json.dumps({"schema_version": 1, "archives": [old]}))
            (root / "latest.json").write_text(json.dumps({"schema_version": 1, **old, "sha256": "f" * 64}))

            self.assertEqual(MODULE.plan_prune_names(root, str(new["archive"]), 2), ())

    def test_requires_room_for_current_and_incoming_generations(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            new = self.add_archive(root, "vapor-platform-recovery-20261102T000000Z-bbbbbbbbbbbb.tar.gz", "b" * 64)
            with self.assertRaises(ValueError):
                MODULE.plan_prune_names(root, str(new["archive"]), 1)

    def test_missing_metadata_keeps_all_preexisting_generations(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            old = self.add_archive(root, "vapor-platform-recovery-20261101T000000Z-aaaaaaaaaaaa.tar.gz", "a" * 64)
            new = self.add_archive(root, "vapor-platform-recovery-20261102T000000Z-bbbbbbbbbbbb.tar.gz", "b" * 64)

            self.assertEqual(MODULE.plan_prune_names(root, str(new["archive"]), 2), ())

    def test_corrupt_current_archive_prevents_pruning(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            old = self.add_archive(root, "vapor-platform-recovery-20261101T000000Z-aaaaaaaaaaaa.tar.gz", "a" * 64)
            current = self.add_archive(root, "vapor-platform-recovery-20261102T000000Z-bbbbbbbbbbbb.tar.gz", "b" * 64)
            incoming = self.add_archive(root, "vapor-platform-recovery-20261103T000000Z-cccccccccccc.tar.gz", "c" * 64)
            archive_path = root / str(current["archive"])
            archive_path.write_bytes(b"x" * archive_path.stat().st_size)
            (root / "index.json").write_text(json.dumps({"schema_version": 1, "archives": [old, current]}))
            (root / "latest.json").write_text(json.dumps({"schema_version": 1, **current}))

            self.assertEqual(MODULE.plan_prune_names(root, str(incoming["archive"]), 2), ())

    def test_symlinked_sidecar_is_not_accepted(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            old = self.add_archive(root, "vapor-platform-recovery-20261101T000000Z-aaaaaaaaaaaa.tar.gz", "a" * 64)
            current = self.add_archive(root, "vapor-platform-recovery-20261102T000000Z-bbbbbbbbbbbb.tar.gz", "b" * 64)
            incoming = self.add_archive(root, "vapor-platform-recovery-20261103T000000Z-cccccccccccc.tar.gz", "c" * 64)
            sidecar = root / f"{old['archive']}.sha256"
            (root / "index.json").write_text(json.dumps({"schema_version": 1, "archives": [old, current]}))
            (root / "latest.json").write_text(json.dumps({"schema_version": 1, **current}))

            original_is_symlink = pathlib.Path.is_symlink

            def report_sidecar_symlink(path: pathlib.Path) -> bool:
                return path == sidecar or original_is_symlink(path)

            with mock.patch.object(pathlib.Path, "is_symlink", report_sidecar_symlink):
                self.assertEqual(MODULE.plan_prune_names(root, str(incoming["archive"]), 2), ())


if __name__ == "__main__":
    unittest.main()
