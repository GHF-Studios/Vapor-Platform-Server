from __future__ import annotations

import importlib.util
import hashlib
import io
import pathlib
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest import mock


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
VALIDATOR_PATH = REPO_ROOT / "deploy" / "scripts" / "state-bundle-validate.py"
SPEC = importlib.util.spec_from_file_location("state_bundle_validate", VALIDATOR_PATH)
validator = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(validator)


def add_file(archive: tarfile.TarFile, name: str, text: str) -> None:
    payload = text.encode("utf-8")
    info = tarfile.TarInfo(name)
    info.size = len(payload)
    archive.addfile(info, io.BytesIO(payload))


def add_dir(archive: tarfile.TarFile, name: str) -> None:
    info = tarfile.TarInfo(name)
    info.type = tarfile.DIRTYPE
    archive.addfile(info)


def make_bundle(members: list[tuple[str, str | None, bytes | None]]) -> pathlib.Path:
    temp_dir = pathlib.Path(tempfile.mkdtemp())
    bundle = temp_dir / "bundle.tar.gz"
    with tarfile.open(bundle, "w:gz") as archive:
        for name, kind, payload in members:
            if kind == "dir":
                add_dir(archive, name)
            elif kind == "symlink":
                info = tarfile.TarInfo(name)
                info.type = tarfile.SYMTYPE
                info.linkname = "target"
                archive.addfile(info)
            else:
                info = tarfile.TarInfo(name)
                payload = payload or b""
                info.size = len(payload)
                archive.addfile(info, io.BytesIO(payload))
    return bundle


ROOT = "vapor-platform-recovery"
REVISION = "a" * 40
VALID_MANIFEST = f"""schema_version = 2
created_at_utc = "2026-11-01T00:00:00Z"
runtime_mode = "monolith"
deploy_root_commit = "{REVISION}"
operating_system = "Ubuntu 24.04.3 LTS"
architecture = "x86_64"
secrets_included = true
archive_encryption = "none"
build_outputs_excluded = true
source_pins_included = true
config_included = true
host_routing_included = true
included_categories = ["state", "config", "host", "source"]
recovery_kit_included = true
""".encode()

KIT_FILES = {
    "recover-fresh-host.sh": b"#!/usr/bin/env bash\necho recovery\n",
    "recovery_bundle_metadata.py": b"#!/usr/bin/env python3\nprint('metadata')\n",
    "state-bundle-validate.py": b"#!/usr/bin/env python3\nprint('validate')\n",
    "deployed_source_manifest.py": b"#!/usr/bin/env python3\nprint('manifest')\n",
    "service_deploy_map.py": b"#!/usr/bin/env python3\nDEPLOY_TARGETS = {}\n",
    "README.txt": b"Recovery instructions\n",
}
KIT_SUMS = "".join(
    f"{hashlib.sha256(payload).hexdigest()}  {name}\n" for name, payload in KIT_FILES.items()
).encode()
V2_KIT_FILES = {
    name: payload
    for name, payload in KIT_FILES.items()
    if name != "service_deploy_map.py"
}
V2_KIT_SUMS = "".join(
    f"{hashlib.sha256(payload).hexdigest()}  {name}\n" for name, payload in V2_KIT_FILES.items()
).encode()
V1_KIT_FILES = {
    name: payload
    for name, payload in KIT_FILES.items()
    if name not in {"state-bundle-validate.py", "deployed_source_manifest.py", "service_deploy_map.py"}
}
V1_KIT_SUMS = "".join(
    f"{hashlib.sha256(payload).hexdigest()}  {name}\n" for name, payload in V1_KIT_FILES.items()
).encode()


def valid_members() -> list[tuple[str, str | None, bytes | None]]:
    return [
        (ROOT, "dir", None),
        (f"{ROOT}/state", "dir", None),
        (f"{ROOT}/manifest.toml", "file", VALID_MANIFEST),
        (f"{ROOT}/state/docs/current.txt", "file", b"release\n"),
        (f"{ROOT}/config", "dir", None),
        (f"{ROOT}/config/vapor-server", "dir", None),
        (f"{ROOT}/config/vapor-server/platform.env", "file", b"VAPOR_DOMAIN=vapor.example.test\n"),
        (f"{ROOT}/source", "dir", None),
        (f"{ROOT}/source/root-commit.txt", "file", (REVISION + "\n").encode()),
        (f"{ROOT}/source/submodules.txt", "file", b""),
        (f"{ROOT}/source/recovery-kit", "dir", None),
        *[(f"{ROOT}/source/recovery-kit/{name}", "file", payload) for name, payload in KIT_FILES.items()],
        (f"{ROOT}/source/recovery-kit/SHA256SUMS", "file", KIT_SUMS),
        (f"{ROOT}/host", "dir", None),
        (f"{ROOT}/host/caddy", "dir", None),
        (f"{ROOT}/host/caddy/Caddyfile", "file", b"vapor.example.test {}\n"),
        (f"{ROOT}/host/systemd", "dir", None),
        (f"{ROOT}/host/systemd/vapor-platform-server.service", "file", b"[Service]\n"),
        (f"{ROOT}/host/systemd/vapor-platform-ops.socket", "file", b"[Socket]\n"),
        (f"{ROOT}/host/systemd/vapor-platform-ops@.service", "file", b"[Service]\n"),
    ]


class StateBundleValidationTests(unittest.TestCase):
    def test_accepts_valid_bundle(self) -> None:
        bundle = make_bundle(valid_members())
        validator.validate_bundle(bundle)

    def test_accepts_previous_recovery_kit_version(self) -> None:
        members = [
            entry for entry in valid_members()
            if not entry[0].endswith("recovery-kit/service_deploy_map.py")
        ]
        members = [
            (name, kind, V2_KIT_SUMS if name.endswith("recovery-kit/SHA256SUMS") else payload)
            for name, kind, payload in members
        ]
        validator.validate_bundle(make_bundle(members))

    def test_accepts_legacy_v1_recovery_kit(self) -> None:
        members = [
            entry for entry in valid_members()
            if not entry[0].endswith((
                "recovery-kit/state-bundle-validate.py",
                "recovery-kit/deployed_source_manifest.py",
                "recovery-kit/service_deploy_map.py",
            ))
        ]
        members = [
            (name, kind, V1_KIT_SUMS if name.endswith("recovery-kit/SHA256SUMS") else payload)
            for name, kind, payload in members
        ]
        validator.validate_bundle(make_bundle(members))

    def test_recovery_kit_validator_runs_with_only_its_bundled_files(self) -> None:
        kit_names = {
            "recover-fresh-host.sh",
            "recovery_bundle_metadata.py",
            "state-bundle-validate.py",
            "deployed_source_manifest.py",
            "service_deploy_map.py",
            "README.txt",
        }
        kit_dir = REPO_ROOT / "deploy" / "scripts"
        kit_payloads = {name: (kit_dir / name).read_bytes() for name in kit_names}
        checksum_text = "".join(
            f"{hashlib.sha256(payload).hexdigest()}  {name}\n"
            for name, payload in kit_payloads.items()
        ).encode()
        members = [
            entry for entry in valid_members()
            if not entry[0].startswith(f"{ROOT}/source/recovery-kit/")
        ]
        members.extend(
            (f"{ROOT}/source/recovery-kit/{name}", "file", payload)
            for name, payload in kit_payloads.items()
        )
        members.append((f"{ROOT}/source/recovery-kit/SHA256SUMS", "file", checksum_text))
        bundle = make_bundle(members)
        with tempfile.TemporaryDirectory() as temporary:
            extracted_kit = pathlib.Path(temporary) / "kit"
            extracted_kit.mkdir()
            with tarfile.open(bundle, "r:gz") as archive:
                for name in kit_names | {"SHA256SUMS"}:
                    member = archive.extractfile(f"{ROOT}/source/recovery-kit/{name}")
                    assert member is not None
                    (extracted_kit / name).write_bytes(member.read())
            result = subprocess.run(
                [sys.executable, str(extracted_kit / "state-bundle-validate.py"), str(bundle)],
                capture_output=True,
                text=True,
                check=False,
            )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_accepts_old_bundle_without_deployed_source_manifest(self) -> None:
        bundle = make_bundle(valid_members())
        validator.validate_bundle(bundle)

    def test_validates_deployed_source_manifest_when_present(self) -> None:
        members = valid_members()
        members.append((
            f"{ROOT}/state/operations/deployed-sources.json",
            "file",
            b'{"schema_version":1,"targets":{"identity":{"composition_commit":"'
            + ("a" * 40).encode()
            + b'","source_commit":"'
            + ("b" * 40).encode()
            + b'","deployed_at_utc":"2026-11-07T00:00:00Z"}}}',
        ))
        validator.validate_bundle(make_bundle(members))

    def test_rejects_invalid_optional_deployed_source_manifest(self) -> None:
        members = valid_members()
        members.append((
            f"{ROOT}/state/operations/deployed-sources.json",
            "file",
            b'{"schema_version":1,"targets":{"identity":{"source_commit":"bad"}}}',
        ))
        with self.assertRaisesRegex(validator.ValidationError, "deployed-source manifest"):
            validator.validate_bundle(make_bundle(members))

    def test_rejects_parent_directory_escape(self) -> None:
        members = valid_members()
        members.append((f"{ROOT}/state/../escape", "file", b"bad"))
        bundle = make_bundle(members)
        with self.assertRaises(validator.ValidationError):
            validator.validate_bundle(bundle)

    def test_rejects_symlink(self) -> None:
        members = valid_members()
        members.append((f"{ROOT}/state/link", "symlink", None))
        bundle = make_bundle(members)
        with self.assertRaises(validator.ValidationError):
            validator.validate_bundle(bundle)

    def test_rejects_secret_manifest(self) -> None:
        members = valid_members()
        members = [
            (name, kind, b"schema_version = 1\nsecrets_included = true\n" if name == f"{ROOT}/manifest.toml" else payload)
            for name, kind, payload in members
        ]
        bundle = make_bundle(members)
        with self.assertRaises(validator.ValidationError):
            validator.validate_bundle(bundle)

    def test_rejects_recovery_kit_hash_mismatch(self) -> None:
        members = valid_members()
        members = [
            (name, kind, b"tampered\n" if name.endswith("recovery-kit/README.txt") else payload)
            for name, kind, payload in members
        ]
        with self.assertRaisesRegex(validator.ValidationError, "recovery kit checksum"):
            validator.validate_bundle(make_bundle(members))

    def test_rejects_partial_recovery_kit(self) -> None:
        members = [entry for entry in valid_members() if not entry[0].endswith("recovery-kit/README.txt")]
        with self.assertRaisesRegex(validator.ValidationError, "recovery kit"):
            validator.validate_bundle(make_bundle(members))

    def test_rejects_a_member_larger_than_the_expanded_size_limit(self) -> None:
        with mock.patch.object(validator, "MAX_MEMBER_BYTES", 32):
            bundle = make_bundle(valid_members() + [(f"{ROOT}/state/large.bin", "file", b"x" * 33)])
            with self.assertRaisesRegex(validator.ValidationError, "member size limit"):
                validator.validate_bundle(bundle)

    def test_rejects_total_expanded_size_before_extracting_members(self) -> None:
        with mock.patch.object(validator, "MAX_TOTAL_BYTES", 256):
            bundle = make_bundle(valid_members())
            with self.assertRaisesRegex(validator.ValidationError, "expanded size limit"):
                validator.validate_bundle(bundle)

    def test_rejects_too_many_archive_members(self) -> None:
        with mock.patch.object(validator, "MAX_MEMBER_COUNT", 4):
            bundle = make_bundle(valid_members())
            with self.assertRaisesRegex(validator.ValidationError, "member count limit"):
                validator.validate_bundle(bundle)


if __name__ == "__main__":
    unittest.main()
