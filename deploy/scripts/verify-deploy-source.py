#!/usr/bin/env python3
"""Validate a Platform revision and its fixed first-party submodule sources."""

import argparse
import configparser
import json
import re
import subprocess


MODULES = {
    name: f"https://github.com/GHF-Studios/{name}.git"
    for name in (
        "Atlas",
        "Vapor-Artifact-Server",
        "Vapor-Build-Cache-Server",
        "Vapor-Diagnostics-Server",
        "Vapor-Docs-Server",
        "Vapor-Homepage-Server",
        "Vapor-Identity-Server",
        "Vapor-Registry-Server",
    )
}


def git(root, *args):
    return subprocess.check_output(["git", "-C", root, *args], text=True).strip()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--tree")
    parser.add_argument("--pins-json")
    args = parser.parse_args()

    if not re.fullmatch(r"[0-9a-fA-F]{40}", args.commit):
        raise ValueError("deployment revision is not a full commit hash")
    actual_tree = git(args.root, "rev-parse", f"{args.commit}^{{tree}}")
    if args.tree and actual_tree.lower() != args.tree.lower():
        raise ValueError("approved tree does not match the selected commit")

    source = subprocess.check_output(
        ["git", "-C", args.root, "show", f"{args.commit}:.gitmodules"], text=True
    )
    config = configparser.RawConfigParser(strict=True)
    config.optionxform = str
    config.read_string(source)
    modules = {}
    for section in config.sections():
        if not section.startswith('submodule "') or not section.endswith('"'):
            raise ValueError("unexpected Git submodule configuration section")
        path = config.get(section, "path", fallback="")
        url = config.get(section, "url", fallback="")
        if path in modules or url != MODULES.get(path):
            raise ValueError(f"unapproved submodule source for {path!r}")
        modules[path] = url
    if modules != MODULES:
        raise ValueError(".gitmodules does not contain the exact approved first-party modules")

    actual_pins = {}
    for line in git(args.root, "ls-tree", args.commit).splitlines():
        metadata, path = line.split("\t", 1)
        mode, kind, revision = metadata.split()
        if mode == "160000" and kind == "commit":
            actual_pins[path] = revision
    if set(actual_pins) != set(MODULES):
        raise ValueError("commit does not pin the exact approved submodule paths")
    if args.pins_json:
        pins = json.loads(args.pins_json)
        requested = {pin["path"]: pin["commit"] for pin in pins}
        if len(requested) != len(pins) or requested != actual_pins:
            raise ValueError("commit submodule gitlinks do not match the approved request")
    print(json.dumps({"commit": args.commit, "tree": actual_tree, "submodules": actual_pins}))


if __name__ == "__main__":
    main()
