#!/usr/bin/env python3
"""Choose a safe Platform deployment scope from changed root paths."""

from __future__ import annotations

import sys
from collections.abc import Iterable

from service_deploy_map import DEPLOY_TARGETS


TARGET_BY_PACKAGE_PATH = {
    record["package_path"]: target for target, record in DEPLOY_TARGETS.items()
}
IGNORED_ROOT_FILES = {
    "README",
    "README.md",
    "CHANGELOG.md",
    "CONTRIBUTING.md",
    "CODE_OF_CONDUCT.md",
    "LICENSE",
    "LICENSE.md",
}
OFFLINE_RECOVERY_PATHS = {
    "deploy/scripts/recover-fresh-host.sh",
    "deploy/scripts/recover-state-only-host.sh",
    "deploy/scripts/backup-state-only-to-usb.ps1",
}


def _is_non_deploy_path(path: str) -> bool:
    return (
        not path
        or path == "docs"
        or path.startswith("docs/")
        or path == ".github"
        or path.startswith(".github/")
        or path.startswith("deploy/tests/")
        or path == "deploy/scripts/deployment_scope.py"
        or path in OFFLINE_RECOVERY_PATHS
        or path.lower().endswith(".md")
        or path in IGNORED_ROOT_FILES
    )


def classify_changed_paths(paths: Iterable[str]) -> str:
    """Return `none`, one service target, or `full` for root/multi-service changes.

    Unknown root paths intentionally select the full composition so new runtime
    inputs cannot silently bypass deployment.
    """
    changed_targets: set[str] = set()
    root_runtime_changed = False

    for raw_path in paths:
        path = raw_path.strip().replace("\\", "/")
        while path.startswith("./"):
            path = path[2:]
        path = path.lstrip("/")
        if _is_non_deploy_path(path):
            continue

        root = path.partition("/")[0]
        target = TARGET_BY_PACKAGE_PATH.get(root)
        if target is not None:
            changed_targets.add(target)
            continue

        root_runtime_changed = True

    if not root_runtime_changed and not changed_targets:
        return "none"
    if not root_runtime_changed and len(changed_targets) == 1:
        return next(iter(changed_targets))
    return "full"


def main() -> int:
    print(classify_changed_paths(sys.stdin))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
