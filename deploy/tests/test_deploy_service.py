from __future__ import annotations

import contextlib
import importlib.util
import io
import os
import pathlib
import sys
import tempfile
import types
import unittest
from unittest import mock


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
SCRIPT_DIR = REPO_ROOT / "deploy" / "scripts"
sys.path.insert(0, str(SCRIPT_DIR))
DEPLOY_PATH = SCRIPT_DIR / "deploy-service.py"
SPEC = importlib.util.spec_from_file_location("deploy_service", DEPLOY_PATH)
DEPLOY = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(DEPLOY)


ROOT_COMMIT = "a" * 40
ROOT_TREE = "e" * 40
SOURCE_COMMIT = "b" * 40
REQUEST = "01234567-89ab-cdef-0123-456789abcdef"


class FakeRunner:
    def __init__(
        self,
        fail_public_calls: set[int] | None = None,
        *,
        load_state: str = "loaded",
        active: bool = True,
        dirty_source: bool = False,
        fail_build: bool = False,
    ):
        self.calls: list[list[str]] = []
        self.public_calls = 0
        self.fail_public_calls = fail_public_calls or set()
        self.load_state = load_state
        self.active = active
        self.dirty_source = dirty_source
        self.fail_build = fail_build

    def run(self, command: list[str], *, env: dict[str, str] | None = None):
        self.calls.append(command)
        stdout = ""
        returncode = 0
        stderr = ""
        if command[0] == "git" and "ls-tree" in command:
            path = command[-1]
            stdout = f"160000 commit {SOURCE_COMMIT}\t{path}"
        elif command[0] == "git" and "rev-parse" in command:
            if any("Vapor-Identity-Server" in part or "Atlas" in part for part in command):
                stdout = SOURCE_COMMIT
            elif "HEAD^{tree}" in command:
                stdout = ROOT_TREE
            else:
                stdout = ROOT_COMMIT
        elif command[0] == "git" and "status" in command:
            stdout = " M Vapor-Identity-Server" if self.dirty_source else ""
        elif command[0] == "systemctl" and "show" in command:
            stdout = self.load_state
        elif command[0] == "systemctl" and "is-active" in command and not self.active:
            returncode = 3
        elif command[0] == "cargo" and self.fail_build:
            returncode = 101
            stderr = "mock build failure"
        elif command[0] == "curl" and command[-1].startswith("https://"):
            self.public_calls += 1
            if self.public_calls in self.fail_public_calls:
                returncode = 22
                stderr = "HTTP 503"
        elif command[0] == "/usr/bin/bash" and command[-1].endswith("deploy-atlas.sh"):
            web_root = pathlib.Path((env or {})["VAPOR_ATLAS_WEB_ROOT"])
            current = web_root / "current"
            release = web_root / "releases" / SOURCE_COMMIT
            release.mkdir(parents=True, exist_ok=True)
            next_link = web_root / "current.next"
            if next_link.exists() or next_link.is_symlink():
                next_link.unlink()
            os.symlink(f"releases/{SOURCE_COMMIT}", next_link)
            os.replace(next_link, current)
        return type("Completed", (), {"returncode": returncode, "stdout": stdout, "stderr": stderr})()


class DeployServiceTests(unittest.TestCase):
    def test_rejects_configured_root_that_crosses_a_symlink(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            real = root / "real"
            real.mkdir()
            linked = root / "linked"
            try:
                linked.symlink_to(real, target_is_directory=True)
            except OSError:
                self.skipTest("platform does not permit creating symlinks for this test")
            with self.assertRaisesRegex(DEPLOY.DeploymentError, "symlink component"):
                DEPLOY.reject_symlink_components(linked / "state", "state root")

    def test_runtime_owner_is_fixed_to_root_and_configured_service_group(self) -> None:
        calls: list[tuple[pathlib.Path, int, int]] = []
        with mock.patch.object(DEPLOY, "grp", types.SimpleNamespace(getgrnam=lambda name: types.SimpleNamespace(gr_gid=4242))), \
             mock.patch.object(DEPLOY.os, "chown", side_effect=lambda path, uid, gid: calls.append((pathlib.Path(path), uid, gid)), create=True):
            DEPLOY.set_runtime_owner_group(pathlib.Path("candidate"), "vapor")
        self.assertEqual(calls, [(pathlib.Path("candidate"), 0, 4242)])

    def test_atomic_copy_assigns_runtime_ownership_only_for_live_executable(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            source = root / "source"
            source.write_bytes(b"binary")
            backup = root / "backup"
            runtime = root / "runtime"
            with mock.patch.object(DEPLOY, "set_runtime_owner_group") as set_owner:
                DEPLOY.atomic_copy(source, backup)
                DEPLOY.atomic_copy(source, runtime, runtime_owner=True)
            self.assertEqual(backup.read_bytes(), b"binary")
            self.assertEqual(runtime.read_bytes(), b"binary")
            self.assertEqual(set_owner.call_count, 1)
            owner_target = set_owner.call_args.args[0]
            self.assertEqual(owner_target.parent, runtime.parent)
            self.assertTrue(owner_target.name.startswith(".runtime."))

    def test_state_snapshot_and_restore_preserve_file_ownership_on_posix(self) -> None:
        if os.name != "posix" or not hasattr(os, "chown"):
            self.skipTest("POSIX ownership metadata is unavailable")
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            source = root / "state"
            source.mkdir(mode=0o750)
            payload = source / "database.sqlite3"
            payload.write_bytes(b"db")
            destination = root / "saved"
            self.assertTrue(DEPLOY.snapshot_tree(source, destination))
            saved_stat = payload.stat()
            backup_stat = (destination / payload.name).stat()
            self.assertEqual((backup_stat.st_uid, backup_stat.st_gid), (saved_stat.st_uid, saved_stat.st_gid))
            payload.write_bytes(b"new")
            DEPLOY.restore_tree(source, destination, True)
            restored_stat = (source / payload.name).stat()
            self.assertEqual((restored_stat.st_uid, restored_stat.st_gid), (saved_stat.st_uid, saved_stat.st_gid))
            self.assertEqual((source / payload.name).read_bytes(), b"db")

    def _fixture(self, runner: FakeRunner):
        temporary = tempfile.TemporaryDirectory()
        root = pathlib.Path(temporary.name)
        source = root / "source"
        package = source / "Vapor-Identity-Server"
        package.mkdir(parents=True)
        (package / "Cargo.toml").write_text("[package]\nname='fixture'\n", encoding="utf-8")
        built = source / "target" / "release" / "vapor_identity_server"
        built.parent.mkdir(parents=True)
        built.write_bytes(b"new binary")
        deploy_root = root / "deploy"
        stable = deploy_root / "target" / "release" / "vapor_identity_server"
        stable.parent.mkdir(parents=True)
        stable.write_bytes(b"old binary")
        state_root = root / "state"
        state = state_root / "identity"
        state.mkdir(parents=True)
        (state / "database.sqlite3").write_bytes(b"old state")
        operations = state_root / "operations"
        operations.mkdir(parents=True)
        manifest = operations / "deployed-sources.json"
        manifest.write_text(
            '{"schema_version":1,"targets":{"identity":{"composition_commit":"'
            + "c" * 40
            + '","source_commit":"'
            + "d" * 40
            + '","deployed_at_utc":"2026-11-07T00:00:00Z"}}}\n',
            encoding="utf-8",
        )
        environment = {
            "VAPOR_APPROVED_SOURCE_ROOT": str(source),
            "VAPOR_DEPLOY_ROOT": str(deploy_root),
            "VAPOR_STATE_ROOT": str(state_root),
            "VAPOR_PLATFORM_DB": str(root / "platform.sqlite3"),
            "VAPOR_APPROVED_DEPLOY_ID": REQUEST,
            "VAPOR_APPROVED_DEPLOY_VERSION": "3",
            "VAPOR_SERVER_MODE": "services",
            "VAPOR_DEPLOYMENT_SCOPE": "service",
            "VAPOR_SERVICE_TARGET": "identity",
            "VAPOR_APPROVED_TREE": ROOT_TREE,
            "VAPOR_DOMAIN": "vapor.example.test",
            "VAPOR_GROUP": "root",
        }
        return temporary, environment, stable, state, manifest, source

    def test_success_builds_and_restarts_only_selected_service(self) -> None:
        runner = FakeRunner()
        temporary, environment, stable, state, manifest, _ = self._fixture(runner)
        try:
            with mock.patch.dict(os.environ, environment, clear=True), \
                 mock.patch.object(DEPLOY, "set_runtime_owner_group"), \
                 contextlib.redirect_stdout(io.StringIO()):
                result = DEPLOY.deploy_service(
                    type("Args", (), {"approved": ROOT_COMMIT, "target": "identity"})(), runner
                )
            self.assertEqual(result, 0)
            self.assertEqual(stable.read_bytes(), b"new binary")
            self.assertEqual((state / "database.sqlite3").read_bytes(), b"old state")
            self.assertIn(ROOT_COMMIT, manifest.read_text(encoding="utf-8"))
            builds = [call for call in runner.calls if call[0] == "cargo"]
            self.assertEqual(len(builds), 1)
            build = builds[0]
            self.assertEqual(build[1:4], ["build", "--release", "--locked"])
            manifest_path = pathlib.Path(build[build.index("--manifest-path") + 1])
            self.assertEqual(manifest_path.parent.name, "Vapor-Identity-Server")
            self.assertEqual(manifest_path.name, "Cargo.toml")
            self.assertEqual(
                pathlib.Path(build[build.index("--target-dir") + 1]),
                pathlib.Path(environment["VAPOR_APPROVED_SOURCE_ROOT"]) / "target",
            )
            self.assertNotIn("--target", build)
            unit_calls = [call[-1] for call in runner.calls if call[0] == "systemctl"]
            self.assertEqual(unit_calls, [
                "vapor-identity.service", "vapor-identity.service",
                "vapor-identity.service", "vapor-identity.service",
            ])
            self.assertFalse((pathlib.Path(environment["VAPOR_STATE_ROOT"]) / "operations" / "scoped-deploy-rollback" / REQUEST).exists())
        finally:
            temporary.cleanup()

    def test_unloaded_or_inactive_target_fails_before_stopping_any_unit(self) -> None:
        for runner in (FakeRunner(load_state="not-found"), FakeRunner(active=False)):
            with self.subTest(runner=runner):
                temporary, environment, _, _, _, _ = self._fixture(runner)
                try:
                    with mock.patch.dict(os.environ, environment, clear=True):
                        with self.assertRaises(DEPLOY.DeploymentError):
                            DEPLOY.deploy_service(
                                type("Args", (), {"approved": ROOT_COMMIT, "target": "identity"})(), runner
                            )
                    self.assertFalse(any(call[0] == "cargo" for call in runner.calls))
                    self.assertFalse(any(call[0] == "systemctl" and call[1] in {"stop", "start"} for call in runner.calls))
                finally:
                    temporary.cleanup()

    def test_dirty_source_and_failed_build_do_not_mutate_the_live_service(self) -> None:
        for runner in (FakeRunner(dirty_source=True), FakeRunner(fail_build=True)):
            with self.subTest(runner=runner):
                temporary, environment, _, _, _, _ = self._fixture(runner)
                try:
                    with mock.patch.dict(os.environ, environment, clear=True):
                        with self.assertRaises(DEPLOY.DeploymentError):
                            DEPLOY.deploy_service(
                                type("Args", (), {"approved": ROOT_COMMIT, "target": "identity"})(), runner
                            )
                    self.assertFalse(any(call[0] == "systemctl" and call[1] in {"stop", "start"} for call in runner.calls))
                finally:
                    temporary.cleanup()

    def test_manifest_write_failure_restores_binary_and_service_state(self) -> None:
        runner = FakeRunner()
        temporary, environment, stable, state, manifest, _ = self._fixture(runner)
        original_manifest = manifest.read_bytes()
        try:
            with mock.patch.dict(os.environ, environment, clear=True), \
                 mock.patch.object(DEPLOY, "set_runtime_owner_group"), \
                 mock.patch.object(DEPLOY, "update_target", side_effect=RuntimeError("manifest disk failure")), \
                 contextlib.redirect_stderr(io.StringIO()):
                result = DEPLOY.deploy_service(
                    type("Args", (), {"approved": ROOT_COMMIT, "target": "identity"})(), runner
                )
            self.assertEqual(result, 2)
            self.assertEqual(stable.read_bytes(), b"old binary")
            self.assertEqual((state / "database.sqlite3").read_bytes(), b"old state")
            self.assertEqual(manifest.read_bytes(), original_manifest)
        finally:
            temporary.cleanup()

    def test_health_failure_restores_binary_state_and_manifest(self) -> None:
        runner = FakeRunner(fail_public_calls={2})
        temporary, environment, stable, state, manifest, _ = self._fixture(runner)
        original_manifest = manifest.read_bytes()
        try:
            with mock.patch.dict(os.environ, environment, clear=True), \
                 mock.patch.object(DEPLOY, "set_runtime_owner_group"), \
                 contextlib.redirect_stderr(io.StringIO()):
                result = DEPLOY.deploy_service(
                    type("Args", (), {"approved": ROOT_COMMIT, "target": "identity"})(), runner
                )
            self.assertEqual(result, 2)
            self.assertEqual(stable.read_bytes(), b"old binary")
            self.assertEqual((state / "database.sqlite3").read_bytes(), b"old state")
            self.assertEqual(manifest.read_bytes(), original_manifest)
            self.assertFalse((state.parent / "operations" / "scoped-deploy-rollback" / REQUEST).exists())
        finally:
            temporary.cleanup()

    def test_rollback_health_failure_preserves_recovery_backup(self) -> None:
        runner = FakeRunner(fail_public_calls={2, 3})
        temporary, environment, stable, _, _, _ = self._fixture(runner)
        try:
            error = io.StringIO()
            with mock.patch.dict(os.environ, environment, clear=True), \
                 mock.patch.object(DEPLOY, "set_runtime_owner_group"), \
                 contextlib.redirect_stderr(error):
                result = DEPLOY.deploy_service(
                    type("Args", (), {"approved": ROOT_COMMIT, "target": "identity"})(), runner
                )
            self.assertEqual(result, 3)
            self.assertIn("rollback incomplete", error.getvalue())
            backup = pathlib.Path(environment["VAPOR_STATE_ROOT"]) / "operations" / "scoped-deploy-rollback" / REQUEST
            self.assertTrue((backup / "previous-binary").is_file())
            self.assertEqual(stable.read_bytes(), b"old binary")
        finally:
            temporary.cleanup()

    def test_state_snapshot_failure_restarts_service_without_swapping_binary(self) -> None:
        runner = FakeRunner()
        temporary, environment, stable, state, _, _ = self._fixture(runner)
        try:
            with mock.patch.dict(os.environ, environment, clear=True), \
                 mock.patch.object(DEPLOY, "set_runtime_owner_group"), \
                 mock.patch.object(DEPLOY, "snapshot_tree", side_effect=DEPLOY.DeploymentError("snapshot copy failed")), \
                 contextlib.redirect_stderr(io.StringIO()):
                result = DEPLOY.deploy_service(
                    type("Args", (), {"approved": ROOT_COMMIT, "target": "identity"})(), runner
                )
            self.assertEqual(result, 2)
            self.assertEqual(stable.read_bytes(), b"old binary")
            self.assertEqual((state / "database.sqlite3").read_bytes(), b"old state")
            lifecycle_calls = [call[1] for call in runner.calls if call[0] == "systemctl" and call[1] in {"stop", "start"}]
            self.assertEqual(lifecycle_calls, ["stop", "stop", "start"])
        finally:
            temporary.cleanup()

    def _atlas_fixture(self):
        temporary = tempfile.TemporaryDirectory()
        root = pathlib.Path(temporary.name)
        source = root / "source"
        (source / "Atlas").mkdir(parents=True)
        deploy_root = root / "deploy"
        state_root = root / "state"
        state_root.mkdir()
        web_root = root / "atlas-web"
        previous_revision = "d" * 40
        (web_root / "releases" / previous_revision).mkdir(parents=True)
        try:
            os.symlink(f"releases/{previous_revision}", web_root / "current")
        except OSError:
            temporary.cleanup()
            self.skipTest("platform does not permit symlink-based Atlas release fixtures")
        environment = {
            "VAPOR_APPROVED_SOURCE_ROOT": str(source),
            "VAPOR_DEPLOY_ROOT": str(deploy_root),
            "VAPOR_STATE_ROOT": str(state_root),
            "VAPOR_PLATFORM_DB": str(root / "platform.sqlite3"),
            "VAPOR_APPROVED_DEPLOY_ID": REQUEST,
            "VAPOR_APPROVED_DEPLOY_VERSION": "3",
            "VAPOR_SERVER_MODE": "services",
            "VAPOR_DEPLOYMENT_SCOPE": "service",
            "VAPOR_SERVICE_TARGET": "atlas",
            "VAPOR_APPROVED_TREE": ROOT_TREE,
            "VAPOR_DOMAIN": "vapor.example.test",
            "VAPOR_ATLAS_WEB_ROOT": str(web_root),
            "VAPOR_GROUP": "root",
        }
        return temporary, environment, web_root, previous_revision

    def test_atlas_scope_publishes_static_release_without_restarting_services(self) -> None:
        runner = FakeRunner()
        temporary, environment, web_root, previous_revision = self._atlas_fixture()
        try:
            with mock.patch.dict(os.environ, environment, clear=True), contextlib.redirect_stdout(io.StringIO()):
                result = DEPLOY.deploy_service(
                    type("Args", (), {"approved": ROOT_COMMIT, "target": "atlas"})(), runner
                )
            self.assertEqual(result, 0)
            self.assertEqual(os.readlink(web_root / "current"), f"releases/{SOURCE_COMMIT}")
            self.assertEqual(runner.public_calls, 2)
            self.assertFalse(any(call[0] == "systemctl" for call in runner.calls))
            manifest = pathlib.Path(environment["VAPOR_STATE_ROOT"]) / "operations" / "deployed-sources.json"
            self.assertIn('"atlas"', manifest.read_text(encoding="utf-8"))
            self.assertNotEqual(os.readlink(web_root / "current"), f"releases/{previous_revision}")
        finally:
            temporary.cleanup()

    def test_atlas_public_health_failure_restores_prior_release(self) -> None:
        runner = FakeRunner(fail_public_calls={2})
        temporary, environment, web_root, previous_revision = self._atlas_fixture()
        try:
            with mock.patch.dict(os.environ, environment, clear=True), contextlib.redirect_stderr(io.StringIO()):
                result = DEPLOY.deploy_service(
                    type("Args", (), {"approved": ROOT_COMMIT, "target": "atlas"})(), runner
                )
            self.assertEqual(result, 2)
            self.assertEqual(os.readlink(web_root / "current"), f"releases/{previous_revision}")
            self.assertFalse((pathlib.Path(environment["VAPOR_STATE_ROOT"]) / "operations" / "scoped-deploy-rollback" / REQUEST).exists())
        finally:
            temporary.cleanup()


if __name__ == "__main__":
    unittest.main()
