from __future__ import annotations

import os
import pathlib
import shlex
import shutil
import subprocess
import sys


def bash_executable() -> str:
    candidates = [shutil.which("bash")]
    if os.name == "nt":
        candidates.append(r"C:\Program Files\Git\bin\bash.exe")
    for candidate in candidates:
        if candidate and pathlib.Path(candidate).is_file():
            return candidate
    raise RuntimeError("Bash is required to run deployment script tests")


def posix_path(value: str | pathlib.Path) -> str:
    path = str(value)
    if os.name != "nt":
        return path
    path = path.replace("\\", "/")
    if len(path) >= 2 and path[1] == ":":
        return f"/{path[0].lower()}{path[2:]}"
    return path


def shell_path_list(fake_bin: pathlib.Path, inherited_path: str) -> str:
    if os.name != "nt":
        return f"{fake_bin}:{inherited_path}"
    bash_dir = pathlib.Path(bash_executable()).parent
    ordered = [
        posix_path(fake_bin),
        posix_path(bash_dir.parent / "usr" / "bin"),
        posix_path(bash_dir.parent / "mingw64" / "bin"),
    ]
    ordered.extend(posix_path(part) for part in inherited_path.split(os.pathsep) if part)
    return ":".join(ordered)


def write_python3_shim(directory: pathlib.Path) -> None:
    if os.name != "nt":
        return
    shim = directory / "python3"
    shim.write_text(
        "#!/bin/sh\nexec " + shlex.quote(posix_path(sys.executable)) + ' "$@"\n',
        encoding="utf-8",
    )
    shim.chmod(0o755)


def read_symlink(path: pathlib.Path) -> str:
    if os.name != "nt":
        return os.readlink(path)
    result = subprocess.run(
        [bash_executable(), "-lc", f"readlink {shlex.quote(posix_path(path))}"],
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout.strip()
