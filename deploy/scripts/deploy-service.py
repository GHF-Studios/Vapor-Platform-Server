#!/usr/bin/env python3
"""Deploy one approved, root-pinned service while preserving its rollback state."""

from __future__ import annotations

import argparse
import os
import pathlib
import re
import shutil
import stat
import subprocess
import sys
import tempfile

try:
    import grp
except ImportError:  # Windows unit tests do not have POSIX group ownership.
    grp = None

from service_deploy_map import target_map
from deployed_source_manifest import update_target


SCRIPT_DIR = pathlib.Path(__file__).resolve().parent
COMMIT = re.compile(r"[0-9a-f]{40}\Z")
REQUEST_ID = re.compile(r"[0-9a-fA-F-]{36}\Z")


class DeploymentError(RuntimeError):
    pass


class CommandRunner:
    def run(self, command: list[str], *, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
        return subprocess.run(command, check=False, capture_output=True, text=True, env=env)


def run_checked(runner: CommandRunner, command: list[str], *, env: dict[str, str] | None = None) -> str:
    result = runner.run(command, env=env)
    if result.returncode != 0:
        detail = (result.stderr or result.stdout or "command failed").strip()
        raise DeploymentError(f"{pathlib.Path(command[0]).name} failed: {detail[:240]}")
    return result.stdout.strip()


def safe_state_child(state_root: pathlib.Path, name: str) -> pathlib.Path:
    if not name or name in {".", ".."} or "/" in name or "\\" in name:
        raise DeploymentError("fixed state target is invalid")
    if state_root.is_symlink() or not state_root.is_absolute():
        raise DeploymentError("state root is unsafe")
    root = reject_symlink_components(state_root, "state root")
    if root == pathlib.Path(root.anchor) or not root.is_absolute():
        raise DeploymentError("state root is unsafe")
    target = root / name
    if target.is_symlink():
        raise DeploymentError("service state directory must not be a symlink")
    return target


def ensure_parent_directory(path: pathlib.Path) -> None:
    path = reject_symlink_components(path, "deployment directory")
    path.mkdir(parents=True, exist_ok=True)
    if path.is_symlink() or not path.is_dir():
        raise DeploymentError("deployment directory is unsafe")


def reject_symlink_components(path: pathlib.Path, label: str) -> pathlib.Path:
    if not path.is_absolute():
        raise DeploymentError(f"{label} must be absolute")
    if ".." in path.parts:
        raise DeploymentError(f"{label} must not contain parent traversal")
    normalized = pathlib.Path(os.path.abspath(path))
    current = pathlib.Path(normalized.anchor)
    for part in normalized.parts[1:]:
        current = current / part
        try:
            mode = current.lstat().st_mode
        except FileNotFoundError:
            continue
        if stat.S_ISLNK(mode):
            raise DeploymentError(f"{label} contains a symlink component")
    return normalized


def public_health(runner: CommandRunner, domain: str, path: str) -> None:
    if not re.fullmatch(r"[A-Za-z0-9.-]+", domain):
        raise DeploymentError("configured public domain is invalid")
    url = f"https://{domain}{path}"
    run_checked(runner, [
        "curl", "--fail", "--silent", "--show-error", "--connect-timeout", "2",
        "--max-time", "8", "--resolve", f"{domain}:443:127.0.0.1", url,
    ])


def check_health(runner: CommandRunner, target: dict[str, str], domain: str) -> None:
    if target["kind"] == "service":
        run_checked(runner, ["curl", "--fail", "--silent", "--show-error", "--connect-timeout", "2", "--max-time", "8", target["loopback_health"]])
    public_health(runner, domain, target["public_health"])


def read_current_link(path: pathlib.Path) -> str:
    if not path.is_symlink():
        raise DeploymentError("Atlas current release is not a symlink")
    link = os.readlink(path)
    if not re.fullmatch(r"releases/[0-9a-f]{40}", link):
        raise DeploymentError("Atlas current release link has an unexpected target")
    if not (path.parent / link).is_dir():
        raise DeploymentError("Atlas current release directory is missing")
    return link


def set_current_link(path: pathlib.Path, link: str) -> None:
    temporary = path.with_name(f".{path.name}.rollback.{os.getpid()}")
    if temporary.exists() or temporary.is_symlink():
        temporary.unlink()
    os.symlink(link, temporary)
    os.replace(temporary, path)


def snapshot_tree(source: pathlib.Path, destination: pathlib.Path) -> bool:
    if source.is_symlink():
        raise DeploymentError("service state path is a symlink")
    if not source.exists():
        return False
    if not source.is_dir():
        raise DeploymentError("service state path is not a directory")
    copy_tree_preserving_ownership(source, destination)
    return True


def restore_tree(live: pathlib.Path, saved: pathlib.Path, existed: bool) -> None:
    if live.is_symlink():
        raise DeploymentError("service state path became a symlink during rollback")
    if live.exists():
        shutil.rmtree(live)
    if existed:
        copy_tree_preserving_ownership(saved, live)


def copy_tree_preserving_ownership(source: pathlib.Path, destination: pathlib.Path) -> None:
    shutil.copytree(source, destination, symlinks=True)
    if os.name != "posix" or not hasattr(os, "chown"):
        return
    for source_root, directories, files in os.walk(source, topdown=True, followlinks=False):
        source_paths = [pathlib.Path(source_root), *(pathlib.Path(source_root) / name for name in directories + files)]
        for source_path in source_paths:
            relative = source_path.relative_to(source)
            copied_path = destination / relative
            metadata = source_path.lstat()
            try:
                os.chown(copied_path, metadata.st_uid, metadata.st_gid, follow_symlinks=False)
                if not stat.S_ISLNK(metadata.st_mode):
                    os.chmod(copied_path, stat.S_IMODE(metadata.st_mode), follow_symlinks=False)
            except (OSError, NotImplementedError) as error:
                raise DeploymentError("could not preserve service-state ownership during snapshot/restore") from error


def set_runtime_owner_group(path: pathlib.Path, group_name: str) -> None:
    if grp is None or not hasattr(os, "chown"):
        return
    try:
        group_id = grp.getgrnam(group_name).gr_gid
        os.chown(path, 0, group_id)
    except (KeyError, OSError) as error:
        raise DeploymentError("could not assign the runtime executable to the service group") from error


def atomic_copy(source: pathlib.Path, destination: pathlib.Path, mode: int = 0o750) -> None:
    if source.is_symlink() or not source.is_file():
        raise DeploymentError("staged executable is missing or unsafe")
    ensure_parent_directory(destination.parent)
    fd, temporary_name = tempfile.mkstemp(prefix=f".{destination.name}.", suffix=".new", dir=destination.parent)
    os.close(fd)
    temporary = pathlib.Path(temporary_name)
    try:
        shutil.copy2(source, temporary)
        os.chmod(temporary, mode)
        set_runtime_owner_group(temporary, os.environ.get("VAPOR_GROUP", "vapor"))
        os.replace(temporary, destination)
    finally:
        if temporary.exists():
            temporary.unlink()


def verify_source(runner: CommandRunner, source_root: pathlib.Path, root_commit: str, target: str, expected_tree: str) -> str:
    if not COMMIT.fullmatch(root_commit):
        raise DeploymentError("approved root revision is invalid")
    if not COMMIT.fullmatch(expected_tree):
        raise DeploymentError("approved root tree is invalid")
    actual_root = run_checked(runner, ["git", "-C", str(source_root), "rev-parse", "HEAD"])
    if actual_root != root_commit:
        raise DeploymentError("temporary source worktree does not match approved root revision")
    actual_tree = run_checked(runner, ["git", "-C", str(source_root), "rev-parse", "HEAD^{tree}"])
    if actual_tree != expected_tree:
        raise DeploymentError("temporary source worktree tree does not match approved composition")
    status = run_checked(runner, ["git", "-C", str(source_root), "status", "--porcelain", "--untracked-files=all"])
    if status:
        raise DeploymentError("temporary source worktree is not clean")
    package_path = source_root / target_map(target)["package_path"]
    if not package_path.exists() or package_path.is_symlink():
        raise DeploymentError("approved target source is missing or unsafe")
    source_commit = run_checked(runner, ["git", "-C", str(package_path), "rev-parse", "HEAD"])
    gitlink = run_checked(runner, ["git", "-C", str(source_root), "ls-tree", root_commit, "--", target_map(target)["package_path"]])
    fields = gitlink.split()
    if len(fields) != 4 or fields[0] != "160000" or fields[1] != "commit" or fields[2] != source_commit:
        raise DeploymentError("checked-out service commit differs from its approved Platform gitlink")
    return source_commit


def verify_queue(runner: CommandRunner, env: dict[str, str], commit: str, target: str) -> None:
    request_id = env.get("VAPOR_APPROVED_DEPLOY_ID", "")
    version = env.get("VAPOR_APPROVED_DEPLOY_VERSION", "")
    if not REQUEST_ID.fullmatch(request_id) or not version.isdigit() or int(version) < 1:
        raise DeploymentError("approved deployment context is missing")
    if env.get("VAPOR_SERVER_MODE") != "services" or env.get("VAPOR_DEPLOYMENT_SCOPE") != "service":
        raise DeploymentError("service deployment requires an approved services-mode request")
    if env.get("VAPOR_SERVICE_TARGET") != target:
        raise DeploymentError("requested service target does not match the deploy argument")
    database = env.get("VAPOR_PLATFORM_DB") or str(pathlib.Path(env.get("VAPOR_PLATFORM_STATE", str(pathlib.Path(env["VAPOR_STATE_ROOT"]) / "platform"))) / "platform.sqlite3")
    run_checked(runner, [
        sys.executable, str(SCRIPT_DIR / "deploy-request-queue.py"),
        "--database", database, "verify", "--id", request_id,
        "--version", version, "--commit", commit,
        "--server-mode", "services", "--deployment-scope", "service",
        "--service-target", target,
    ])


def deploy_atlas(runner: CommandRunner, source_root: pathlib.Path, root_commit: str, source_commit: str, env: dict[str, str], backup: pathlib.Path) -> None:
    web_root = reject_symlink_components(
        pathlib.Path(env.get("VAPOR_ATLAS_WEB_ROOT", "/srv/vapor-web/atlas")),
        "Atlas web root",
    )
    reject_symlink_components(web_root / "releases", "Atlas release directory")
    current = web_root / "current"
    ensure_parent_directory(web_root)
    previous_link = read_current_link(current)
    (backup / "atlas-current-link.txt").write_text(previous_link + "\n", encoding="ascii")
    domain = env.get("VAPOR_DOMAIN", "vapor.ghf-studios.site")
    public_health(runner, domain, "/atlas/")
    build_env = dict(env)
    build_env["VAPOR_DEPLOY_ROOT"] = str(source_root)
    command = ["/usr/bin/bash", str(source_root / "deploy" / "scripts" / "deploy-atlas.sh")]
    result = runner.run(command, env=build_env)
    if result.returncode == 0:
        try:
            public_health(runner, domain, "/atlas/")
            update_target(
                pathlib.Path(env["VAPOR_STATE_ROOT"]) / "operations" / "deployed-sources.json",
                "atlas", root_commit, source_commit,
            )
            return
        except Exception as error:
            reason = str(error)
    else:
        reason = (result.stderr or result.stdout or "Atlas publication failed").strip()[:240]
    try:
        set_current_link(current, previous_link)
        public_health(runner, domain, "/atlas/")
    except Exception as rollback_error:
        raise DeploymentError(f"rollback incomplete for atlas; backup preserved at {backup}: {rollback_error}")
    raise DeploymentError(f"atlas deployment failed and prior release was restored: {reason}")


def deploy_service(args: argparse.Namespace, runner: CommandRunner | None = None) -> int:
    runner = runner or CommandRunner()
    env = dict(os.environ)
    source_root = pathlib.Path(env.get("VAPOR_APPROVED_SOURCE_ROOT", ""))
    deploy_root = reject_symlink_components(
        pathlib.Path(env.get("VAPOR_DEPLOY_ROOT", "/opt/vapor-platform-server")),
        "deployment root",
    )
    state_root = reject_symlink_components(
        pathlib.Path(env.get("VAPOR_STATE_ROOT", "/var/lib/vapor-server")),
        "state root",
    )
    if not source_root.is_absolute() or not source_root.is_dir() or source_root.is_symlink():
        raise DeploymentError("approved detached source worktree is missing or unsafe")
    target = target_map(args.target)
    verify_queue(runner, env, args.approved, args.target)
    source_commit = verify_source(
        runner,
        source_root,
        args.approved,
        args.target,
        env.get("VAPOR_APPROVED_TREE", ""),
    )
    request_id = env["VAPOR_APPROVED_DEPLOY_ID"]
    backup_root = state_root / "operations" / "scoped-deploy-rollback"
    ensure_parent_directory(backup_root)
    backup = backup_root / request_id
    if backup.exists() or backup.is_symlink():
        raise DeploymentError("rollback backup path already exists")
    backup.mkdir(mode=0o700)
    mutation_started = False
    rollback_needed = False
    state_path: pathlib.Path | None = None
    state_existed = False
    state_snapshot_complete = False
    binary_path: pathlib.Path | None = None
    previous_manifest: pathlib.Path | None = None
    manifest = state_root / "operations" / "deployed-sources.json"

    if target["kind"] == "static":
        try:
            deploy_atlas(runner, source_root, args.approved, source_commit, env, backup)
            shutil.rmtree(backup)
            return 0
        except DeploymentError as error:
            if "rollback incomplete" in str(error):
                print(str(error), file=sys.stderr)
                return 3
            shutil.rmtree(backup, ignore_errors=True)
            print(str(error), file=sys.stderr)
            return 2 if "prior release was restored" in str(error) else 1

    try:
        load_state = run_checked(runner, ["systemctl", "show", "--property=LoadState", "--value", target["unit"]])
        if load_state != "loaded":
            raise DeploymentError(f"{target['unit']} is not loaded")
        run_checked(runner, ["systemctl", "is-active", "--quiet", target["unit"]])
        check_health(runner, target, env.get("VAPOR_DOMAIN", "vapor.ghf-studios.site"))

        package_manifest = source_root / target["package_path"] / "Cargo.toml"
        if not package_manifest.is_file() or package_manifest.is_symlink():
            raise DeploymentError("approved service Cargo manifest is missing or unsafe")
        run_checked(runner, [
            "cargo", "build", "--release", "--locked", "--manifest-path", str(package_manifest),
            "--target-dir", str(source_root / "target"),
        ])
        built = source_root / "target" / "release" / target["binary"]
        release_dir = deploy_root / "target" / "release"
        binary_path = release_dir / target["binary"]
        reject_symlink_components(binary_path, "service executable path")
        if binary_path.is_symlink() or not binary_path.is_file():
            raise DeploymentError("currently deployed service executable is missing or unsafe")
        atomic_copy(binary_path, backup / "previous-binary")
        state_path = safe_state_child(state_root, target["state_name"])

        rollback_needed = True
        run_checked(runner, ["systemctl", "stop", target["unit"]])
        if target["state_backup"] == "snapshot":
            state_existed = snapshot_tree(state_path, backup / "previous-state")
            state_snapshot_complete = True
        elif target["state_backup"] != "preserve-in-place":
            raise DeploymentError("service state backup policy is invalid")
        if manifest.exists() or manifest.is_symlink():
            if manifest.is_symlink() or not manifest.is_file():
                raise DeploymentError("deployed-source manifest path is unsafe")
            shutil.copy2(manifest, backup / "previous-manifest.json")
            previous_manifest = backup / "previous-manifest.json"
        atomic_copy(built, binary_path)
        mutation_started = True
        run_checked(runner, ["systemctl", "start", target["unit"]])
        check_health(runner, target, env.get("VAPOR_DOMAIN", "vapor.ghf-studios.site"))
        update_target(manifest, args.target, args.approved, source_commit)
        shutil.rmtree(backup)
        print(f"deploy-service: {args.target} deployed from {source_commit}")
        return 0
    except Exception as error:
        if not rollback_needed:
            shutil.rmtree(backup, ignore_errors=True)
            raise
        rollback_errors: list[str] = []
        try:
            run_checked(runner, ["systemctl", "stop", target["unit"]])
        except Exception as rollback_error:
            rollback_errors.append(f"stop failed: {rollback_error}")
        try:
            if binary_path is None:
                raise DeploymentError("previous binary path was not recorded")
            atomic_copy(backup / "previous-binary", binary_path)
        except Exception as rollback_error:
            rollback_errors.append(f"binary restore failed: {rollback_error}")
        try:
            if state_path is not None and state_snapshot_complete:
                restore_tree(state_path, backup / "previous-state", state_existed)
        except Exception as rollback_error:
            rollback_errors.append(f"state restore failed: {rollback_error}")
        try:
            if previous_manifest is not None:
                atomic_copy(previous_manifest, manifest, mode=0o640)
            elif mutation_started and manifest.exists():
                manifest.unlink()
        except Exception as rollback_error:
            rollback_errors.append(f"manifest restore failed: {rollback_error}")
        try:
            run_checked(runner, ["systemctl", "start", target["unit"]])
            check_health(runner, target, env.get("VAPOR_DOMAIN", "vapor.ghf-studios.site"))
        except Exception as rollback_error:
            rollback_errors.append(f"service recovery failed: {rollback_error}")
        if rollback_errors:
            print(f"deploy-service: rollback incomplete for {args.target}; backup preserved at {backup}; " + "; ".join(rollback_errors), file=sys.stderr)
            return 3
        shutil.rmtree(backup)
        print(f"deploy-service: {args.target} failed; prior executable and state restored: {error}", file=sys.stderr)
        return 2


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--approved", required=True)
    parser.add_argument("--target", required=True, choices=sorted({"atlas", "homepage", "docs", "identity", "diagnostics", "registry", "artifact", "build-cache"}))
    args = parser.parse_args()
    try:
        if hasattr(os, "geteuid") and os.geteuid() != 0:
            raise DeploymentError("service deployment must run as root")
        if not COMMIT.fullmatch(args.approved):
            raise DeploymentError("approved revision is invalid")
        return deploy_service(args)
    except Exception as error:
        print(f"deploy-service: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
