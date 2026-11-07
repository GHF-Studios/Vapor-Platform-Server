#!/usr/bin/env python3
"""Allowlisted deploy metadata; values are consumed only by the root deployer."""

from __future__ import annotations

import argparse
import json


DEPLOY_TARGETS = {
    "atlas": {
        "kind": "static",
        "package_path": "Atlas",
        "binary": "",
        "unit": "",
        "loopback_health": "",
        "public_health": "/atlas/",
        "state_name": "",
        "state_backup": "none",
    },
    "homepage": {
        "kind": "service",
        "package_path": "Vapor-Homepage-Server",
        "binary": "vapor_homepage_server",
        "unit": "vapor-homepage.service",
        "loopback_health": "http://127.0.0.1:7111/healthz",
        "public_health": "/healthz",
        "state_name": "homepage",
        "state_backup": "snapshot",
    },
    "docs": {
        "kind": "service",
        "package_path": "Vapor-Docs-Server",
        "binary": "vapor_docs_server",
        "unit": "vapor-docs.service",
        "loopback_health": "http://127.0.0.1:7112/healthz",
        "public_health": "/docs/healthz",
        "state_name": "docs",
        "state_backup": "snapshot",
    },
    "identity": {
        "kind": "service",
        "package_path": "Vapor-Identity-Server",
        "binary": "vapor_identity_server",
        "unit": "vapor-identity.service",
        "loopback_health": "http://127.0.0.1:7113/healthz",
        "public_health": "/api/identity/healthz",
        "state_name": "identity",
        "state_backup": "snapshot",
    },
    "diagnostics": {
        "kind": "service",
        "package_path": "Vapor-Diagnostics-Server",
        "binary": "vapor_diagnostics_server",
        "unit": "vapor-diagnostics.service",
        "loopback_health": "http://127.0.0.1:7114/healthz",
        "public_health": "/api/diagnostics/healthz",
        "state_name": "diagnostics",
        "state_backup": "snapshot",
    },
    "registry": {
        "kind": "service",
        "package_path": "Vapor-Registry-Server",
        "binary": "vapor_registry_server",
        "unit": "vapor-registry.service",
        "loopback_health": "http://127.0.0.1:7115/healthz",
        "public_health": "/api/registry/healthz",
        "state_name": "registry",
        "state_backup": "snapshot",
    },
    "artifact": {
        "kind": "service",
        "package_path": "Vapor-Artifact-Server",
        "binary": "vapor_artifact_server",
        "unit": "vapor-artifact.service",
        "loopback_health": "http://127.0.0.1:7116/api/updates/healthz",
        "public_health": "/api/updates/healthz",
        "state_name": "updates",
        "state_backup": "preserve-in-place",
    },
    "build-cache": {
        "kind": "service",
        "package_path": "Vapor-Build-Cache-Server",
        "binary": "vapor_build_cache_server",
        "unit": "vapor-build-cache.service",
        "loopback_health": "http://127.0.0.1:7117/healthz",
        "public_health": "/api/build-cache/healthz",
        "state_name": "build-cache",
        "state_backup": "preserve-in-place",
    },
}


def target_map(target: str) -> dict[str, str]:
    try:
        return DEPLOY_TARGETS[target]
    except KeyError as error:
        raise ValueError("deployment target is not allowlisted") from error


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("target", choices=sorted(DEPLOY_TARGETS))
    parser.add_argument("--format", choices=("json", "tsv"), default="json")
    args = parser.parse_args()
    record = target_map(args.target)
    if args.format == "tsv":
        print("\t".join(record[key] for key in (
            "kind", "package_path", "binary", "unit", "loopback_health",
            "public_health", "state_name",
            "state_backup",
        )))
    else:
        print(json.dumps(record, separators=(",", ":"), sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
