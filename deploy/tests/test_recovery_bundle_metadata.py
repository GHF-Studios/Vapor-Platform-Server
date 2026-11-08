from __future__ import annotations

import dataclasses
import hashlib
import importlib.util
import io
import pathlib
import sys
import tarfile
import tempfile
import unittest


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
HELPER = REPO_ROOT / "deploy" / "scripts" / "recovery_bundle_metadata.py"
SPEC = importlib.util.spec_from_file_location("recovery_bundle_metadata", HELPER)
METADATA = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = METADATA
SPEC.loader.exec_module(METADATA)


ROOT = "vapor-platform-recovery"
COMMIT = "a" * 40


def _file(archive: tarfile.TarFile, name: str, content: bytes) -> None:
    info = tarfile.TarInfo(name)
    info.size = len(content)
    archive.addfile(info, io.BytesIO(content))


def make_bundle(path: pathlib.Path, *, mode: str = "services", source_commit: str = COMMIT,
                toolchain: bool = True, duplicate_manifest: bool = False,
                manifest_mode: str | None = None) -> None:
    fields = [
        "schema_version = 2",
        f'runtime_mode = "{manifest_mode or mode}"',
        f'deploy_root_commit = "{COMMIT}"',
        'operating_system = "Ubuntu 24.04.3 LTS"',
        'architecture = "x86_64"',
    ]
    if toolchain:
        fields.extend([
            'rustc_version = "1.90.0"',
            'cargo_version = "1.90.0"',
            'rust_host_triple = "x86_64-unknown-linux-gnu"',
        ])
    with tarfile.open(path, "w:gz") as archive:
        _file(archive, f"{ROOT}/config/vapor-server/platform.env", b"VAPOR_DOMAIN=vapor.example.test\n")
        _file(archive, f"{ROOT}/source/root-commit.txt", (source_commit + "\n").encode())
        _file(archive, f"{ROOT}/manifest.toml", ("\n".join(fields) + "\n").encode())
        if duplicate_manifest:
            _file(archive, f"{ROOT}/manifest.toml", ("\n".join(fields) + "\n").encode())


class RecoveryBundleMetadataTests(unittest.TestCase):
    def test_recovery_guide_requires_the_bundle_recorded_ubuntu_release(self) -> None:
        guide = " ".join(
            (REPO_ROOT / "deploy" / "scripts" / "README.txt")
            .read_text(encoding="utf-8")
            .split()
        )

        self.assertIn("same Ubuntu release recorded in the archive", guide)
        self.assertNotIn("Ubuntu 24.04 or newer", guide)

    def test_archive_checksum_requires_one_entry_naming_the_selected_bundle(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            bundle = pathlib.Path(temporary) / "recovery.tar.gz"
            bundle.write_bytes(b"the selected archive")
            digest = hashlib.sha256(bundle.read_bytes()).hexdigest()
            sidecar = pathlib.Path(f"{bundle}.sha256")
            sidecar.write_text(f"{digest}  {bundle.name}\n", encoding="ascii")

            METADATA.verify_archive_checksum(bundle)

    def test_archive_checksum_rejects_sidecar_that_names_another_file(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            bundle = pathlib.Path(temporary) / "recovery.tar.gz"
            bundle.write_bytes(b"the selected archive")
            digest = hashlib.sha256(bundle.read_bytes()).hexdigest()
            pathlib.Path(f"{bundle}.sha256").write_text(
                f"{digest}  different.tar.gz\n", encoding="ascii"
            )

            with self.assertRaisesRegex(METADATA.MetadataError, "names a different archive"):
                METADATA.verify_archive_checksum(bundle)

    def test_archive_checksum_rejects_multiple_entries_and_wrong_digest(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            bundle = pathlib.Path(temporary) / "recovery.tar.gz"
            bundle.write_bytes(b"the selected archive")
            digest = hashlib.sha256(bundle.read_bytes()).hexdigest()
            sidecar = pathlib.Path(f"{bundle}.sha256")
            sidecar.write_text(
                f"{digest}  {bundle.name}\n{digest}  {bundle.name}\n", encoding="ascii"
            )
            with self.assertRaisesRegex(METADATA.MetadataError, "exactly one entry"):
                METADATA.verify_archive_checksum(bundle)

            sidecar.write_text(f"{'0' * 64}  {bundle.name}\n", encoding="ascii")
            with self.assertRaisesRegex(METADATA.MetadataError, "does not match"):
                METADATA.verify_archive_checksum(bundle)

    def test_archive_checksum_bounds_sidecar_read(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            bundle = pathlib.Path(temporary) / "recovery.tar.gz"
            bundle.write_bytes(b"the selected archive")
            pathlib.Path(f"{bundle}.sha256").write_bytes(
                b"0" * (METADATA.MAX_CHECKSUM_SIDECAR_BYTES + 1)
            )

            with self.assertRaisesRegex(METADATA.MetadataError, "exceeds its size limit"):
                METADATA.verify_archive_checksum(bundle)

    def test_services_mode_requires_each_independent_service_binary(self) -> None:
        self.assertEqual(
            METADATA.runtime_binaries("services"),
            (
                "vapor_homepage_server",
                "vapor_docs_server",
                "vapor_identity_server",
                "vapor_diagnostics_server",
                "vapor_registry_server",
                "vapor_artifact_server",
                "vapor_build_cache_server",
                "vapor_platform_server",
            ),
        )
        self.assertEqual(METADATA.build_binaries("services")[0], "vapor_platform_ops")

    def test_monolith_mode_requires_only_its_runtime_binary(self) -> None:
        self.assertEqual(METADATA.runtime_binaries("monolith"), ("vapor_platform_server",))

    def test_reads_exact_revision_mode_and_observed_toolchain(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            bundle = pathlib.Path(temporary) / "recovery.tar.gz"
            make_bundle(bundle)

            metadata = METADATA.read_recovery_metadata(bundle)

        self.assertEqual(metadata.runtime_mode, "services")
        self.assertEqual(metadata.root_commit, COMMIT)
        self.assertEqual(metadata.rustc_version, "1.90.0")
        self.assertEqual(metadata.cargo_version, "1.90.0")
        self.assertEqual(metadata.domain, "vapor.example.test")
        self.assertEqual(metadata.architecture, "x86_64")
        self.assertEqual(metadata.operating_system, "Ubuntu 24.04.3 LTS")

    def test_validates_host_compatibility_before_recovery_mutations(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            bundle = pathlib.Path(temporary) / "recovery.tar.gz"
            make_bundle(bundle)
            metadata = METADATA.read_recovery_metadata(bundle)

        METADATA.validate_host_compatibility(
            metadata, "24.04", "x86_64", "x86_64-unknown-linux-gnu"
        )
        for host in [
            ("22.04", "x86_64", "x86_64-unknown-linux-gnu"),
            ("24.04", "aarch64", "x86_64-unknown-linux-gnu"),
            ("24.04", "x86_64", "aarch64-unknown-linux-gnu"),
        ]:
            with self.subTest(host=host):
                with self.assertRaises(METADATA.MetadataError):
                    METADATA.validate_host_compatibility(metadata, *host)
        for incomplete in [
            dataclasses.replace(metadata, rustc_version=None),
            dataclasses.replace(metadata, cargo_version=None),
        ]:
            with self.subTest(metadata=incomplete):
                with self.assertRaises(METADATA.MetadataError):
                    METADATA.validate_host_compatibility(
                        incomplete, "24.04", "x86_64", "x86_64-unknown-linux-gnu"
                    )

    def test_rejects_bundle_and_manifest_revision_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            bundle = pathlib.Path(temporary) / "recovery.tar.gz"
            make_bundle(bundle, source_commit="b" * 40)

            with self.assertRaises(METADATA.MetadataError):
                METADATA.read_recovery_metadata(bundle)

    def test_rejects_duplicate_manifest_entries(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            bundle = pathlib.Path(temporary) / "recovery.tar.gz"
            make_bundle(bundle, duplicate_manifest=True)

            with self.assertRaises(METADATA.MetadataError):
                METADATA.read_recovery_metadata(bundle)

    def test_reads_pinned_commit_before_full_bundle_validation(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            bundle = pathlib.Path(temporary) / "recovery.tar.gz"
            make_bundle(bundle)

            revision = METADATA.read_bundle_source_commit(bundle)

        self.assertEqual(revision, COMMIT)

    def test_rejects_manifest_and_source_commit_disagreement_during_bootstrap_read(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            bundle = pathlib.Path(temporary) / "recovery.tar.gz"
            make_bundle(bundle, source_commit="b" * 40)

            with self.assertRaises(METADATA.MetadataError):
                METADATA.read_bundle_source_commit(bundle)

    def test_rejects_unsupported_mode_before_build(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            bundle = pathlib.Path(temporary) / "recovery.tar.gz"
            make_bundle(bundle, manifest_mode="anything")

            with self.assertRaises(METADATA.MetadataError):
                METADATA.read_recovery_metadata(bundle)

    def test_accepts_older_schema_v2_bundle_without_toolchain_observation(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            bundle = pathlib.Path(temporary) / "recovery.tar.gz"
            make_bundle(bundle, toolchain=False)

            metadata = METADATA.read_recovery_metadata(bundle)

        self.assertIsNone(metadata.rustc_version)
        self.assertIsNone(metadata.cargo_version)

    def test_provider_readiness_requires_github_only_when_configured(self) -> None:
        status = {
            "steam_identity_ready": True,
            "github_client_id_configured": False,
            "github_client_secret_configured": False,
            "github_browser_login_ready": False,
            "dashboard_ready": True,
        }
        self.assertEqual(METADATA.missing_provider_readiness(status), ())

    def test_provider_readiness_requires_configured_github_login(self) -> None:
        status = {
            "steam_identity_ready": True,
            "github_client_id_configured": True,
            "github_client_secret_configured": False,
            "github_browser_login_ready": False,
            "dashboard_ready": True,
        }
        self.assertEqual(
            METADATA.missing_provider_readiness(status),
            ("github_browser_login_ready",),
        )

    def test_provider_readiness_fails_closed_when_github_configuration_is_unknown(self) -> None:
        status = {
            "steam_identity_ready": True,
            "github_browser_login_ready": False,
            "dashboard_ready": True,
        }
        self.assertEqual(
            METADATA.missing_provider_readiness(status),
            ("github_client_id_configured", "github_client_secret_configured"),
        )

    def test_provider_readiness_requires_steam_and_boolean_dashboard(self) -> None:
        status = {
            "steam_identity_ready": False,
            "github_client_id_configured": False,
            "github_client_secret_configured": False,
            "github_browser_login_ready": False,
            "dashboard_ready": "true",
        }
        self.assertEqual(
            METADATA.missing_provider_readiness(status),
            ("steam_identity_ready", "dashboard_ready"),
        )


if __name__ == "__main__":
    unittest.main()
