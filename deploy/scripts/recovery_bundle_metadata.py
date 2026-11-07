#!/usr/bin/env python3
"""Read bounded, non-secret rebuild pins from a recovery bundle."""

from __future__ import annotations

import dataclasses
import json
import pathlib
import re
import sys
import tarfile
import tomllib


ROOT = "vapor-platform-recovery"
MAX_MANIFEST_BYTES = 64 * 1024
MAX_REVISION_BYTES = 256


class MetadataError(RuntimeError):
    pass


def missing_provider_readiness(status: object) -> tuple[str, ...]:
    if not isinstance(status, dict):
        return (
            "steam_identity_ready",
            "github_client_id_configured",
            "github_client_secret_configured",
            "dashboard_ready",
        )

    missing = [
        key
        for key in ("steam_identity_ready", "dashboard_ready")
        if status.get(key) is not True
    ]
    github_configuration = (
        "github_client_id_configured",
        "github_client_secret_configured",
    )
    unknown_configuration = [
        key for key in github_configuration if type(status.get(key)) is not bool
    ]
    missing.extend(unknown_configuration)
    if not unknown_configuration and any(status[key] for key in github_configuration):
        if status.get("github_browser_login_ready") is not True:
            missing.append("github_browser_login_ready")
    return tuple(missing)


@dataclasses.dataclass(frozen=True)
class RecoveryMetadata:
    runtime_mode: str
    root_commit: str
    rustc_version: str | None
    cargo_version: str | None
    rust_host_triple: str | None
    domain: str
    architecture: str
    operating_system: str


def runtime_binaries(runtime_mode: str) -> tuple[str, ...]:
    if runtime_mode == "monolith":
        return ("vapor_platform_server",)
    if runtime_mode == "services":
        return (
            "vapor_homepage_server",
            "vapor_docs_server",
            "vapor_identity_server",
            "vapor_diagnostics_server",
            "vapor_registry_server",
            "vapor_artifact_server",
            "vapor_build_cache_server",
            "vapor_platform_server",
        )
    raise MetadataError("bundle runtime mode is unsupported")


def build_binaries(runtime_mode: str) -> tuple[str, ...]:
    return ("vapor_platform_ops", *runtime_binaries(runtime_mode))


def _read_member(archive: tarfile.TarFile, expected_name: str, limit: int) -> bytes:
    selected: tarfile.TarInfo | None = None
    for member in archive:
        name = member.name
        while name.startswith("./"):
            name = name[2:]
        if name != expected_name:
            continue
        if selected is not None:
            raise MetadataError("bundle contains a duplicate rebuild metadata entry")
        if not member.isfile() or member.issym() or member.islnk():
            raise MetadataError("rebuild metadata entry is not a regular file")
        selected = member
    if selected is None:
        raise MetadataError("bundle is missing required rebuild metadata")
    stream = archive.extractfile(selected)
    if stream is None:
        raise MetadataError("rebuild metadata entry cannot be read")
    contents = stream.read(limit + 1)
    if len(contents) > limit:
        raise MetadataError("rebuild metadata entry exceeds its size limit")
    return contents


def read_bundle_source_commit(path: pathlib.Path) -> str:
    """Read the root revision needed to fetch the bundle's pinned validator."""
    try:
        with tarfile.open(path, "r:gz") as archive:
            manifest_bytes = _read_member(archive, f"{ROOT}/manifest.toml", MAX_MANIFEST_BYTES)
            revision_bytes = _read_member(archive, f"{ROOT}/source/root-commit.txt", MAX_REVISION_BYTES)
        manifest = tomllib.loads(manifest_bytes.decode("utf-8", errors="strict"))
        revision = revision_bytes.decode("ascii", errors="strict").strip()
    except (OSError, tarfile.TarError, UnicodeDecodeError, tomllib.TOMLDecodeError) as error:
        raise MetadataError("bundle bootstrap source pin is unreadable") from error
    declared = manifest.get("deploy_root_commit")
    if manifest.get("schema_version") != 2:
        raise MetadataError("bundle has an unsupported recovery schema")
    if not isinstance(declared, str) or not re.fullmatch(r"[0-9a-f]{40}", declared):
        raise MetadataError("bundle manifest source revision is invalid")
    if not re.fullmatch(r"[0-9a-f]{40}", revision) or revision != declared:
        raise MetadataError("bundle manifest and source revision do not match")
    return revision


def read_recovery_metadata(path: pathlib.Path) -> RecoveryMetadata:
    if not path.is_file():
        raise MetadataError("bundle path is not a regular file")
    try:
        with tarfile.open(path, "r:gz") as archive:
            manifest_bytes = _read_member(archive, f"{ROOT}/manifest.toml", MAX_MANIFEST_BYTES)
            revision_bytes = _read_member(archive, f"{ROOT}/source/root-commit.txt", MAX_REVISION_BYTES)
            platform_env = _read_member(
                archive, f"{ROOT}/config/vapor-server/platform.env", MAX_MANIFEST_BYTES
            )
    except (OSError, tarfile.TarError, UnicodeDecodeError) as error:
        raise MetadataError("bundle rebuild metadata is unreadable") from error

    try:
        manifest = tomllib.loads(manifest_bytes.decode("utf-8", errors="strict"))
        root_commit = revision_bytes.decode("ascii", errors="strict").strip()
    except (tomllib.TOMLDecodeError, UnicodeDecodeError) as error:
        raise MetadataError("bundle rebuild metadata is malformed") from error

    if manifest.get("schema_version") != 2:
        raise MetadataError("bundle has an unsupported recovery schema")
    runtime_mode = manifest.get("runtime_mode")
    if runtime_mode not in {"monolith", "services"}:
        raise MetadataError("bundle runtime mode is unsupported")
    declared_commit = manifest.get("deploy_root_commit")
    if not isinstance(declared_commit, str) or not re.fullmatch(r"[0-9a-f]{40}", declared_commit):
        raise MetadataError("bundle manifest source revision is invalid")
    if not re.fullmatch(r"[0-9a-f]{40}", root_commit) or root_commit != declared_commit:
        raise MetadataError("bundle manifest and source revision do not match")

    architecture = manifest.get("architecture")
    operating_system = manifest.get("operating_system")
    if architecture not in {"x86_64", "aarch64"}:
        raise MetadataError("bundle host architecture is missing or unsupported")
    if not isinstance(operating_system, str) or not re.fullmatch(
        r"Ubuntu [0-9]+\.[0-9]+(?:\.[0-9]+)?(?: LTS)?", operating_system
    ):
        raise MetadataError("bundle operating-system observation is missing or unsupported")

    domain_values = re.findall(
        rb"(?m)^VAPOR_DOMAIN=(?:\"([A-Za-z0-9.-]+)\"|([A-Za-z0-9.-]+))\s*$",
        platform_env,
    )
    if len(domain_values) != 1:
        raise MetadataError("bundle must contain exactly one valid platform domain")
    domain = next(value for value in domain_values[0] if value).decode("ascii")
    if len(domain) > 253 or any(
        not re.fullmatch(r"[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?", label)
        for label in domain.split(".")
    ):
        raise MetadataError("bundle platform domain is invalid")

    toolchain_fields = (
        manifest.get("rustc_version"),
        manifest.get("cargo_version"),
        manifest.get("rust_host_triple"),
    )
    if any(value is not None for value in toolchain_fields):
        rustc_version, cargo_version, rust_host_triple = toolchain_fields
        if not isinstance(rustc_version, str) or not re.fullmatch(r"\d+\.\d+\.\d+", rustc_version):
            raise MetadataError("bundle Rust compiler observation is invalid")
        if not isinstance(cargo_version, str) or not re.fullmatch(r"\d+\.\d+\.\d+", cargo_version):
            raise MetadataError("bundle Cargo observation is invalid")
        if not isinstance(rust_host_triple, str) or not re.fullmatch(
            r"[A-Za-z0-9_]+(?:-[A-Za-z0-9_]+){2,}", rust_host_triple
        ):
            raise MetadataError("bundle compiler host observation is invalid")
    else:
        rustc_version = cargo_version = rust_host_triple = None

    return RecoveryMetadata(
        runtime_mode=runtime_mode,
        root_commit=root_commit,
        rustc_version=rustc_version,
        cargo_version=cargo_version,
        rust_host_triple=rust_host_triple,
        domain=domain,
        architecture=architecture,
        operating_system=operating_system,
    )


def main(argv: list[str]) -> int:
    if len(argv) == 3 and argv[1] == "--source-commit":
        try:
            print(read_bundle_source_commit(pathlib.Path(argv[2])))
        except MetadataError as error:
            print(f"recovery-metadata: {error}", file=sys.stderr)
            return 1
        return 0
    if len(argv) == 3 and argv[1] == "--check-provider-readiness":
        try:
            status = json.loads(pathlib.Path(argv[2]).read_text(encoding="utf-8"))
        except (OSError, UnicodeDecodeError, json.JSONDecodeError):
            print("recovery-readiness: identity status is unreadable", file=sys.stderr)
            return 1
        missing = missing_provider_readiness(status)
        if missing:
            print("recovery-readiness: missing required readiness: " + ", ".join(missing), file=sys.stderr)
            return 1
        github_configured = status["github_client_id_configured"] or status["github_client_secret_configured"]
        providers = "Steam and dashboard are configured and ready"
        if github_configured:
            providers += "; GitHub browser login is configured and ready"
        else:
            providers += "; GitHub is not configured and remains optional"
        print("recovery-readiness: " + providers)
        return 0
    if len(argv) == 3 and argv[1] == "--describe":
        try:
            metadata = read_recovery_metadata(pathlib.Path(argv[2]))
        except MetadataError as error:
            print(f"recovery-metadata: {error}", file=sys.stderr)
            return 1
        print(json.dumps(dataclasses.asdict(metadata), sort_keys=True))
        return 0
    print(
        "usage: recovery_bundle_metadata.py --describe BUNDLE | "
        "--source-commit BUNDLE | --check-provider-readiness STATUS_JSON",
        file=sys.stderr,
    )
    return 2


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
