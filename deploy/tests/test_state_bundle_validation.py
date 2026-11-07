from __future__ import annotations

import importlib.util
import io
import pathlib
import tarfile
import tempfile
import unittest


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
""".encode()


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
        (f"{ROOT}/host", "dir", None),
        (f"{ROOT}/host/caddy", "dir", None),
        (f"{ROOT}/host/caddy/Caddyfile", "file", b"vapor.example.test {}\n"),
        (f"{ROOT}/host/systemd", "dir", None),
        (f"{ROOT}/host/systemd/vapor-platform-server.service", "file", b"[Service]\n"),
    ]


class StateBundleValidationTests(unittest.TestCase):
    def test_accepts_valid_bundle(self) -> None:
        bundle = make_bundle(valid_members())
        validator.validate_bundle(bundle)

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


if __name__ == "__main__":
    unittest.main()
