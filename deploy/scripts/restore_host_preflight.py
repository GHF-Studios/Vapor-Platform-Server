#!/usr/bin/env python3
"""Reject managed host paths that transactional restore cannot roll back safely."""

from __future__ import annotations

import argparse
import pathlib
import sys


class HostStateError(RuntimeError):
    pass


def validate_host_state(systemd_dir: pathlib.Path, caddyfile: pathlib.Path) -> None:
    if systemd_dir.is_symlink() or (systemd_dir.exists() and not systemd_dir.is_dir()):
        raise HostStateError("systemd unit directory is not a regular directory")
    if systemd_dir.is_dir():
        for path in systemd_dir.iterdir():
            if not (path.name.startswith("vapor-") and path.suffix in {".service", ".timer"}):
                continue
            if path.is_symlink() or not path.is_file():
                raise HostStateError("a managed Vapor unit is not a regular file")

    parent = caddyfile.parent
    if parent.is_symlink() or (parent.exists() and not parent.is_dir()):
        raise HostStateError("Caddy configuration directory is not a regular directory")
    if caddyfile.is_symlink() or (caddyfile.exists() and not caddyfile.is_file()):
        raise HostStateError("Caddy configuration is not a regular file")


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--systemd-dir", type=pathlib.Path, default=pathlib.Path("/etc/systemd/system"))
    parser.add_argument("--caddyfile", type=pathlib.Path, default=pathlib.Path("/etc/caddy/Caddyfile"))
    arguments = parser.parse_args(argv)
    try:
        validate_host_state(arguments.systemd_dir, arguments.caddyfile)
    except OSError:
        print("restore-preflight: managed host files could not be inspected", file=sys.stderr)
        return 1
    except HostStateError as error:
        print(f"restore-preflight: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
