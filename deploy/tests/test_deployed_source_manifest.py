from __future__ import annotations

import importlib.util
import json
import pathlib
import sys
import tempfile
import unittest
from unittest import mock


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
MANIFEST_MODULE = REPO_ROOT / "deploy" / "scripts" / "deployed_source_manifest.py"
sys.path.insert(0, str(MANIFEST_MODULE.parent))
SPEC = importlib.util.spec_from_file_location("deployed_source_manifest", MANIFEST_MODULE)
MANIFEST = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(MANIFEST)


class DeployedSourceManifestTests(unittest.TestCase):
    def test_update_preserves_other_targets_and_validates_revisions(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            path = pathlib.Path(temporary) / "operations" / "deployed-sources.json"
            MANIFEST.update_target(path, "identity", "a" * 40, "b" * 40)
            MANIFEST.update_target(path, "atlas", "c" * 40, "d" * 40)

            data = MANIFEST.validate_manifest_text(path.read_text(encoding="utf-8"))
            self.assertEqual(set(data["targets"]), {"identity", "atlas"})
            self.assertEqual(data["targets"]["identity"]["composition_commit"], "a" * 40)
            self.assertEqual(data["targets"]["atlas"]["source_commit"], "d" * 40)

    def test_full_composition_update_replaces_all_entries_from_git_pins(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary) / "root"
            root.mkdir()
            for mapping in MANIFEST.DEPLOY_TARGETS.values():
                if mapping["package_path"] != ".":
                    (root / mapping["package_path"]).mkdir()
            source_commits = {
                mapping["package_path"]: f"{index:040x}"
                for index, mapping in enumerate(MANIFEST.DEPLOY_TARGETS.values(), start=1)
            }

            def fake_git(command, text=True):
                if command[-2:] == ["rev-parse", "HEAD"] and pathlib.Path(command[2]).resolve() == root.resolve():
                    return "a" * 40 + "\n"
                if command[-2:] == ["rev-parse", "HEAD"]:
                    package_name = pathlib.Path(command[2]).name
                    return source_commits[package_name] + "\n"
                if "ls-tree" in command:
                    package_path = command[-1]
                    return f"160000 commit {source_commits[package_path]}\t{package_path}\n"
                raise AssertionError(command)

            manifest = root / "operations" / "deployed-sources.json"
            with mock.patch.object(MANIFEST.subprocess, "check_output", side_effect=fake_git):
                MANIFEST.update_all_from_root(manifest, root)
            data = MANIFEST.validate_manifest_text(manifest.read_text(encoding="utf-8"))
            self.assertEqual(set(data["targets"]), set(MANIFEST.DEPLOY_TARGETS))
            self.assertEqual(data["targets"]["atlas"]["composition_commit"], "a" * 40)

    def test_full_composition_update_rejects_submodule_pin_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary) / "root"
            root.mkdir()
            for mapping in MANIFEST.DEPLOY_TARGETS.values():
                if mapping["package_path"] != ".":
                    (root / mapping["package_path"]).mkdir()

            def fake_git(command, text=True):
                if command[-2:] == ["rev-parse", "HEAD"] and pathlib.Path(command[2]).resolve() == root.resolve():
                    return "a" * 40 + "\n"
                if command[-2:] == ["rev-parse", "HEAD"]:
                    return "b" * 40 + "\n"
                if "ls-tree" in command:
                    package_path = command[-1]
                    return f"160000 commit {'c' * 40}\t{package_path}\n"
                raise AssertionError(command)

            with mock.patch.object(MANIFEST.subprocess, "check_output", side_effect=fake_git):
                with self.assertRaisesRegex(MANIFEST.ManifestError, "does not match the Platform pin"):
                    MANIFEST.update_all_from_root(root / "ops" / "manifest.json", root)

    def test_rejects_unknown_targets_malformed_entries_and_symlink_path(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            path = pathlib.Path(temporary) / "manifest.json"
            with self.assertRaisesRegex(MANIFEST.ManifestError, "allowlisted"):
                MANIFEST.update_target(path, "unregistered", "a" * 40, "b" * 40)
            with self.assertRaisesRegex(MANIFEST.ManifestError, "full lowercase"):
                MANIFEST.update_target(path, "identity", "A" * 40, "b" * 40)

            malformed = {
                "schema_version": 1,
                "targets": {
                    "identity": {
                        "composition_commit": "a" * 40,
                        "source_commit": "b" * 40,
                        "deployed_at_utc": "not-a-timeZ",
                    }
                },
            }
            with self.assertRaisesRegex(MANIFEST.ManifestError, "timestamp"):
                MANIFEST.validate_manifest_data(malformed)

            if hasattr(pathlib.Path, "symlink_to"):
                real = pathlib.Path(temporary) / "real.json"
                real.write_text(json.dumps({"schema_version": 1, "targets": {}}), encoding="utf-8")
                try:
                    path.symlink_to(real)
                except OSError:
                    self.skipTest("platform does not permit creating symlinks for this test")
                with self.assertRaisesRegex(MANIFEST.ManifestError, "symlink"):
                    MANIFEST.update_target(path, "identity", "a" * 40, "b" * 40)

    def test_rejects_symlinked_parent_before_creating_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            real = root / "real-operations"
            real.mkdir()
            linked = root / "operations"
            try:
                linked.symlink_to(real, target_is_directory=True)
            except OSError:
                self.skipTest("platform does not permit creating symlinks for this test")
            with self.assertRaisesRegex(MANIFEST.ManifestError, "symlink"):
                MANIFEST.update_target(linked / "manifest.json", "identity", "a" * 40, "b" * 40)
            self.assertFalse((real / "manifest.json").exists())


if __name__ == "__main__":
    unittest.main()
