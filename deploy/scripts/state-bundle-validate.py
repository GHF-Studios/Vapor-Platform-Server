#!/usr/bin/env python3
"""Validate a Vapor full-platform recovery bundle without revealing contents."""

from __future__ import annotations

import pathlib
import re
import sys
import tarfile
import tomllib


class ValidationError(RuntimeError):
    pass


ROOT = "vapor-platform-recovery"
REQUIRED_CATEGORIES = {"state", "config", "host", "source"}


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

    try:
        with tarfile.open(path, "r:gz") as archive:
            for member in archive:
                parts = validate_member(member)
                normalized_name = "/".join(parts)
                if normalized_name in names:
                    raise ValidationError("bundle contains a duplicate archive entry")
                names.add(normalized_name)

                if normalized_name == f"{ROOT}/manifest.toml":
                    manifest_member = member
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
            declared_categories, runtime_mode, declared_root_commit, declared_toolchain = validate_manifest_text(
                manifest_file.read(64 * 1024).decode("utf-8", errors="strict")
            )

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
            if not declared_categories.issubset(categories_seen):
                raise ValidationError("bundle category declaration does not match its contents")
    except (tarfile.TarError, UnicodeDecodeError) as error:
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
