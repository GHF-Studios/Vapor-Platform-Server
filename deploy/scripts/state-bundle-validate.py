#!/usr/bin/env python3
"""Validate a Vapor full-platform recovery bundle without revealing contents."""

from __future__ import annotations

import hashlib
import pathlib
import re
import sys
import tarfile
import tomllib

# This validator is also shipped inside the USB recovery kit. Running it there
# must not write interpreter bytecode into the source-pinned directory, where
# the sync tool correctly treats unlisted files as tampering.
sys.dont_write_bytecode = True
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from deployed_source_manifest import ManifestError, validate_manifest_text as validate_deployed_source_manifest


class ValidationError(RuntimeError):
    pass


ROOT = "vapor-platform-recovery"
REQUIRED_CATEGORIES = {"state", "config", "host", "source"}
RECOVERY_KIT_V1_FILES = {
    "recover-fresh-host.sh",
    "recovery_bundle_metadata.py",
    "README.txt",
    "SHA256SUMS",
}
RECOVERY_KIT_V2_FILES = RECOVERY_KIT_V1_FILES | {
    "state-bundle-validate.py",
    "deployed_source_manifest.py",
}
RECOVERY_KIT_V3_FILES = RECOVERY_KIT_V2_FILES | {"service_deploy_map.py"}
# Restore extracts the validated bundle to a temporary directory before applying
# state. Bound both archive complexity and expanded disk use before extraction.
MAX_MEMBER_COUNT = 100_000
MAX_MEMBER_BYTES = 2 * 1024 * 1024 * 1024
MAX_TOTAL_BYTES = 8 * 1024 * 1024 * 1024
STATE_ONLY_REQUIRED_FILES = {
    f"{ROOT}/state/identity/identity.sqlite3",
    f"{ROOT}/state/platform/platform.sqlite3",
    f"{ROOT}/state/registry/registry.sqlite3",
}


def normalized_parts(name: str) -> tuple[str, ...]:
    while name.startswith("./"):
        name = name[2:]
    path = pathlib.PurePosixPath(name)
    if path.is_absolute() or not path.parts:
        raise ValidationError("bundle contains an absolute or empty path")
    if any(part in {"", ".", ".."} for part in path.parts):
        raise ValidationError("bundle contains an unsafe path component")
    return path.parts


def validate_member(member: tarfile.TarInfo) -> tuple[str, ...]:
    parts = normalized_parts(member.name)
    if parts[0] != ROOT:
        raise ValidationError("bundle contains an entry outside the recovery root")
    if member.issym() or member.islnk():
        raise ValidationError("bundle contains a link entry")
    if member.isdev() or member.isfifo():
        raise ValidationError("bundle contains a special-file entry")
    if not (member.isdir() or member.isfile()):
        raise ValidationError("bundle contains an unsupported entry type")
    return parts


def validate_manifest_text(
    text: str,
) -> tuple[set[str], str, str, tuple[str, str, str] | None]:
    try:
        manifest = tomllib.loads(text)
    except tomllib.TOMLDecodeError as error:
        raise ValidationError("bundle manifest is not valid TOML") from error

    if manifest.get("schema_version") != 2:
        raise ValidationError("bundle has unsupported recovery schema")
    if manifest.get("secrets_included") is not True:
        raise ValidationError("bundle does not declare included plaintext credentials")
    if manifest.get("archive_encryption") != "none":
        raise ValidationError("bundle encryption declaration is invalid")
    if manifest.get("build_outputs_excluded") is not True:
        raise ValidationError("bundle does not declare rebuildable outputs excluded")
    if manifest.get("source_pins_included") is not True:
        raise ValidationError("bundle does not declare source pins included")
    if manifest.get("config_included") is not True:
        raise ValidationError("bundle does not declare configuration included")
    if manifest.get("recovery_kit_included") not in {None, True}:
        raise ValidationError("bundle recovery kit declaration is invalid")
    categories = manifest.get("included_categories")
    if not isinstance(categories, list) or not all(isinstance(item, str) for item in categories):
        raise ValidationError("bundle category declaration is invalid")
    if not REQUIRED_CATEGORIES.issubset(set(categories)):
        raise ValidationError("bundle does not declare all recovery categories")
    root_commit = manifest.get("deploy_root_commit", "")
    if not isinstance(root_commit, str) or not re.fullmatch(r"[0-9a-f]{40}", root_commit):
        raise ValidationError("bundle manifest source revision is invalid")
    toolchain_values = (
        manifest.get("rustc_version"),
        manifest.get("cargo_version"),
        manifest.get("rust_host_triple"),
    )
    if any(value is not None for value in toolchain_values):
        rustc_version, cargo_version, host_triple = toolchain_values
        if not isinstance(rustc_version, str) or not re.fullmatch(r"\d+\.\d+\.\d+", rustc_version):
            raise ValidationError("bundle Rust compiler version is invalid")
        if not isinstance(cargo_version, str) or not re.fullmatch(r"\d+\.\d+\.\d+", cargo_version):
            raise ValidationError("bundle Cargo version is invalid")
        if not isinstance(host_triple, str) or not re.fullmatch(
            r"[A-Za-z0-9_]+(?:-[A-Za-z0-9_]+){2,}", host_triple
        ):
            raise ValidationError("bundle Rust host triple is invalid")
        toolchain = (rustc_version, cargo_version, host_triple)
    else:
        toolchain = None
    return set(categories), manifest.get("runtime_mode", ""), root_commit, toolchain


def validate_state_only_manifest(manifest: dict[str, object]) -> None:
    """Reject ambiguous or host-bearing schema-v3 snapshots."""
    expected_keys = {
        "schema_version", "created_at_utc", "state_catalog_version",
        "secrets_included", "archive_encryption", "build_outputs_excluded",
        "config_included", "host_routing_included", "source_pins_included",
        "recovery_kit_included", "included_categories",
    }
    if set(manifest) != expected_keys:
        raise ValidationError("state-only manifest contains unsupported fields")
    if manifest.get("state_catalog_version") != 1:
        raise ValidationError("state-only catalog version is unsupported")
    if manifest.get("secrets_included") is not False:
        raise ValidationError("state-only archive must declare secrets excluded")
    if manifest.get("archive_encryption") != "none":
        raise ValidationError("state-only archive encryption declaration is invalid")
    if manifest.get("build_outputs_excluded") is not True:
        raise ValidationError("state-only archive must exclude rebuildable outputs")
    if any(manifest.get(key) is not False for key in (
        "config_included", "host_routing_included", "source_pins_included",
        "recovery_kit_included",
    )):
        raise ValidationError("state-only archive declares host/configuration material")
    if manifest.get("included_categories") != ["state"]:
        raise ValidationError("state-only archive must declare only the state category")
    created = manifest.get("created_at_utc")
    if not isinstance(created, str) or not re.fullmatch(
        r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z", created
    ):
        raise ValidationError("state-only creation timestamp is invalid")


def validate_state_only_members(
    archive: tarfile.TarFile,
    members: dict[str, tarfile.TarInfo],
    manifest: dict[str, object],
) -> None:
    validate_state_only_manifest(manifest)
    allowed_exact = STATE_ONLY_REQUIRED_FILES | {
        f"{ROOT}/state/operations/deployed-sources.json",
    }
    allowed_prefixes = (
        f"{ROOT}/state/docs/current/",
        f"{ROOT}/state/docs/releases/",
        f"{ROOT}/state/updates/releases/",
    )
    allowed_dirs = {ROOT, f"{ROOT}/state"}
    for file_path in allowed_exact:
        parts = pathlib.PurePosixPath(file_path).parts
        for size in range(1, len(parts)):
            allowed_dirs.add("/".join(parts[:size]))
    for prefix in allowed_prefixes:
        parts = pathlib.PurePosixPath(prefix.rstrip("/")).parts
        for size in range(1, len(parts) + 1):
            allowed_dirs.add("/".join(parts[:size]))

    for name, member in members.items():
        if name in {f"{ROOT}/manifest.toml"}:
            continue
        if member.isdir() and (
            name in allowed_dirs or any(name.startswith(prefix) for prefix in allowed_prefixes)
        ):
            continue
        if name in allowed_exact and member.isfile():
            continue
        if any(name.startswith(prefix) and member.isfile() for prefix in allowed_prefixes):
            continue
        # SQLite's WAL and shared-memory companions are part of the database
        # snapshot only when their owning service has one of the allowlisted DBs.
        if member.isfile() and any(
            name in {f"{db}-wal", f"{db}-shm"} for db in STATE_ONLY_REQUIRED_FILES
        ):
            continue
        raise ValidationError(f"state-only archive contains a non-state or unclassified path: {name}")

    missing = STATE_ONLY_REQUIRED_FILES - {
        name for name, member in members.items() if member.isfile()
    }
    if missing:
        raise ValidationError("state-only archive is missing a required service database")
    deployed_sources = members.get(f"{ROOT}/state/operations/deployed-sources.json")
    if deployed_sources is not None:
        stream = archive.extractfile(deployed_sources)
        if stream is None:
            raise ValidationError("state-only deployed-source manifest cannot be read")
        try:
            validate_deployed_source_manifest(stream.read(64 * 1024).decode("utf-8", errors="strict"))
        except (ManifestError, UnicodeDecodeError) as error:
            raise ValidationError("state-only deployed-source manifest is invalid") from error


def validate_bundle(path: pathlib.Path) -> None:
    if not path.is_file():
        raise ValidationError("bundle path is not a file")

    names: set[str] = set()
    categories_seen: set[str] = set()
    manifest_member: tarfile.TarInfo | None = None
    saw_state_file = False
    saw_config_file = False
    saw_root_commit = False
    saw_submodule_pins = False
    root_commit_member: tarfile.TarInfo | None = None
    toolchain_member: tarfile.TarInfo | None = None
    submodule_member: tarfile.TarInfo | None = None
    saw_caddyfile = False
    service_units: set[str] = set()
    deployed_sources_member: tarfile.TarInfo | None = None
    recovery_kit_members: dict[str, tarfile.TarInfo] = {}
    archive_members: dict[str, tarfile.TarInfo] = {}
    member_count = 0
    total_bytes = 0

    try:
        with tarfile.open(path, "r:gz") as archive:
            for member in archive:
                member_count += 1
                if member_count > MAX_MEMBER_COUNT:
                    raise ValidationError("bundle exceeds the archive member count limit")
                if member.isfile():
                    if member.size < 0 or member.size > MAX_MEMBER_BYTES:
                        raise ValidationError("bundle member exceeds the expanded member size limit")
                    total_bytes += member.size
                    if total_bytes > MAX_TOTAL_BYTES:
                        raise ValidationError("bundle exceeds the total expanded size limit")
                parts = validate_member(member)
                normalized_name = "/".join(parts)
                if normalized_name in names:
                    raise ValidationError("bundle contains a duplicate archive entry")
                names.add(normalized_name)
                archive_members[normalized_name] = member

                if normalized_name == f"{ROOT}/manifest.toml":
                    manifest_member = member
                if normalized_name == f"{ROOT}/state/operations/deployed-sources.json" and member.isfile():
                    deployed_sources_member = member
                if len(parts) >= 2 and parts[1] in REQUIRED_CATEGORIES:
                    categories_seen.add(parts[1])
                if len(parts) > 2 and parts[1] == "state" and member.isfile():
                    saw_state_file = True
                if len(parts) > 3 and parts[1:3] == ("config", "vapor-server") and member.isfile():
                    saw_config_file = True
                if normalized_name == f"{ROOT}/source/root-commit.txt" and member.isfile():
                    saw_root_commit = True
                    root_commit_member = member
                if normalized_name == f"{ROOT}/source/toolchain.txt" and member.isfile():
                    toolchain_member = member
                if normalized_name == f"{ROOT}/source/submodules.txt" and member.isfile():
                    saw_submodule_pins = True
                    submodule_member = member
                kit_prefix = f"{ROOT}/source/recovery-kit/"
                if normalized_name.startswith(kit_prefix) and member.isfile():
                    recovery_kit_members[normalized_name[len(kit_prefix):]] = member
                if normalized_name == f"{ROOT}/host/caddy/Caddyfile" and member.isfile():
                    saw_caddyfile = True
                if (
                    len(parts) == 4
                    and parts[1:3] == ("host", "systemd")
                    and member.isfile()
                ):
                    service_units.add(parts[3])

            if manifest_member is None:
                raise ValidationError("bundle is missing its manifest")
            manifest_file = archive.extractfile(manifest_member)
            if manifest_file is None:
                raise ValidationError("bundle manifest cannot be read")
            manifest_text = manifest_file.read(64 * 1024).decode("utf-8", errors="strict")
            manifest_values = tomllib.loads(manifest_text)

            if manifest_values.get("schema_version") == 3:
                validate_state_only_members(archive, archive_members, manifest_values)
                return

            declared_categories, runtime_mode, declared_root_commit, declared_toolchain = validate_manifest_text(
                manifest_text
            )
            manifest_stream = archive.extractfile(manifest_member)
            assert manifest_stream is not None
            manifest_values = tomllib.loads(manifest_stream.read(64 * 1024).decode("utf-8", errors="strict"))

            if deployed_sources_member is not None:
                deployed_sources_stream = archive.extractfile(deployed_sources_member)
                if deployed_sources_stream is None:
                    raise ValidationError("deployed-source manifest cannot be read")
                try:
                    validate_deployed_source_manifest(
                        deployed_sources_stream.read(64 * 1024).decode("utf-8", errors="strict")
                    )
                except (ManifestError, UnicodeDecodeError) as error:
                    raise ValidationError("bundle deployed-source manifest is invalid") from error

            if recovery_kit_members or manifest_values.get("recovery_kit_included") is True:
                if manifest_values.get("recovery_kit_included") is not True:
                    raise ValidationError("bundle contains an undeclared recovery kit")
                kit_files = set(recovery_kit_members)
                if frozenset(kit_files) not in {
                    frozenset(RECOVERY_KIT_V1_FILES),
                    frozenset(RECOVERY_KIT_V2_FILES),
                    frozenset(RECOVERY_KIT_V3_FILES),
                }:
                    raise ValidationError("bundle recovery kit is incomplete")
                checksums_member = recovery_kit_members["SHA256SUMS"]
                checksums_stream = archive.extractfile(checksums_member)
                if checksums_stream is None or checksums_member.size > 4096:
                    raise ValidationError("bundle recovery kit checksum file is invalid")
                checksums_text = checksums_stream.read(4097).decode("ascii", errors="strict")
                checksums: dict[str, str] = {}
                for line in checksums_text.splitlines():
                    match = re.fullmatch(r"([0-9a-f]{64})  ([A-Za-z0-9_.-]+)", line)
                    if match is None or match.group(2) in checksums:
                        raise ValidationError("bundle recovery kit checksum file is malformed")
                    checksums[match.group(2)] = match.group(1)
                expected_kit_files = kit_files - {"SHA256SUMS"}
                if set(checksums) != expected_kit_files:
                    raise ValidationError("bundle recovery kit checksum list is incomplete")
                for name in expected_kit_files:
                    member = recovery_kit_members[name]
                    if member.size > 1024 * 1024:
                        raise ValidationError("bundle recovery kit file exceeds its size limit")
                    stream = archive.extractfile(member)
                    if stream is None or hashlib.sha256(stream.read(1024 * 1024 + 1)).hexdigest() != checksums[name]:
                        raise ValidationError("bundle recovery kit checksum does not match")

            if not REQUIRED_CATEGORIES.issubset(categories_seen):
                raise ValidationError("bundle is missing a required recovery category")
            if not saw_state_file:
                raise ValidationError("bundle is missing state files")
            if not saw_config_file:
                raise ValidationError("bundle is missing server configuration files")
            if not saw_root_commit or not saw_submodule_pins:
                raise ValidationError("bundle is missing source revision pins")
            root_commit_stream = archive.extractfile(root_commit_member)
            submodule_stream = archive.extractfile(submodule_member)
            if root_commit_stream is None or submodule_stream is None:
                raise ValidationError("bundle source revision pins cannot be read")
            root_commit = root_commit_stream.read(256).decode("ascii", errors="strict").strip()
            submodule_status = submodule_stream.read(4 * 1024 * 1024).decode("utf-8", errors="strict")
            if not re.fullmatch(r"[0-9a-f]{40}", root_commit):
                raise ValidationError("bundle root source revision is invalid")
            if root_commit != declared_root_commit:
                raise ValidationError("bundle manifest and source revision do not match")
            if (toolchain_member is None) != (declared_toolchain is None):
                raise ValidationError("bundle toolchain manifest and source record do not match")
            if toolchain_member is not None and declared_toolchain is not None:
                toolchain_stream = archive.extractfile(toolchain_member)
                if toolchain_stream is None:
                    raise ValidationError("bundle toolchain record cannot be read")
                toolchain_text = toolchain_stream.read(4096).decode("ascii", errors="strict")
                fields: dict[str, str] = {}
                for line in toolchain_text.splitlines():
                    key, separator, value = line.partition("=")
                    if not separator or key in fields or not value:
                        raise ValidationError("bundle toolchain record is malformed")
                    fields[key] = value
                if set(fields) != {"rustc_version", "cargo_version", "host_triple"}:
                    raise ValidationError("bundle toolchain record is incomplete")
                if tuple(fields[key] for key in ("rustc_version", "cargo_version", "host_triple")) != declared_toolchain:
                    raise ValidationError("bundle toolchain manifest and source record do not match")
            for line in submodule_status.splitlines():
                if not re.fullmatch(r" [0-9a-f]{40} .+", line):
                    raise ValidationError("bundle contains incomplete or mismatched submodule pins")
            if not saw_caddyfile or not service_units:
                raise ValidationError("bundle is missing host routing or service definitions")
            if runtime_mode == "monolith":
                expected_units = {"vapor-platform-server.service"}
            elif runtime_mode == "services":
                expected_units = {
                    "vapor-homepage.service",
                    "vapor-docs.service",
                    "vapor-identity.service",
                    "vapor-diagnostics.service",
                    "vapor-registry.service",
                    "vapor-artifact.service",
                    "vapor-build-cache.service",
                    "vapor-platform-server.service",
                }
            else:
                raise ValidationError("bundle runtime mode is unsupported")
            if not expected_units.issubset(service_units):
                raise ValidationError("bundle is missing service units for its runtime mode")
            if not {
                "vapor-platform-ops.socket",
                "vapor-platform-ops@.service",
            }.issubset(service_units):
                raise ValidationError("bundle is missing the local root-operations socket units")
            if not declared_categories.issubset(categories_seen):
                raise ValidationError("bundle category declaration does not match its contents")
    except (tarfile.TarError, UnicodeDecodeError, tomllib.TOMLDecodeError) as error:
        raise ValidationError("bundle is not a readable recovery archive") from error


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print("usage: state-bundle-validate.py BUNDLE", file=sys.stderr)
        return 2
    try:
        validate_bundle(pathlib.Path(argv[1]))
    except ValidationError as error:
        print(f"recovery-bundle-validate: {error}", file=sys.stderr)
        return 1
    print("recovery-bundle-validate: structure and manifest are valid")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
