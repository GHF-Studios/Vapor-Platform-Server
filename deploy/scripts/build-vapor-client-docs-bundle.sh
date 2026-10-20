#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'USAGE'
usage: build-vapor-client-docs-bundle.sh --vapor-client PATH [--output PATH]

Builds a curated Vapor Client docs site bundle as a tar.gz archive suitable for
uploading to Vapor-Docs-Server /current.tar.gz.

The bundle intentionally excludes .agents, raw planning intake, local state,
targets, provider toolchains, credentials, and other non-public operational
material.
USAGE
}

VAPOR_CLIENT=""
OUTPUT=""

while [ "$#" -gt 0 ]; do
  case "$1" in
    --vapor-client)
      if [ "$#" -lt 2 ]; then
        usage
        exit 2
      fi
      VAPOR_CLIENT="$2"
      shift 2
      ;;
    --output)
      if [ "$#" -lt 2 ]; then
        usage
        exit 2
      fi
      OUTPUT="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      usage
      exit 2
      ;;
  esac
done

if [ -z "${VAPOR_CLIENT}" ]; then
  usage
  exit 2
fi

VAPOR_CLIENT="$(cd -- "${VAPOR_CLIENT}" && pwd)"
if [ ! -f "${VAPOR_CLIENT}/App.vapor.toml" ]; then
  echo "error: ${VAPOR_CLIENT} does not look like Vapor Client source" >&2
  exit 1
fi

if [ -z "${OUTPUT}" ]; then
  OUTPUT="$(mktemp --suffix=.tar.gz vapor-client-docs.XXXXXXXXXX)"
else
  OUTPUT="$(realpath -m -- "${OUTPUT}")"
fi

if [ -e "${OUTPUT}" ] && [ -s "${OUTPUT}" ]; then
  echo "error: output already exists and is not empty: ${OUTPUT}" >&2
  exit 1
fi

TMP_DIR="$(mktemp -d)"
SITE="${TMP_DIR}/site"

cleanup() {
  rm -rf "${TMP_DIR}"
}
trap cleanup EXIT

install -d -m 0755 "${SITE}"
install -d -m 0755 "${SITE}/client"
install -d -m 0755 "${SITE}/shell"
install -d -m 0755 "${SITE}/vapor"

copy_if_present() {
  local source="$1"
  local target="$2"
  if [ -e "${source}" ]; then
    mkdir -p "$(dirname -- "${target}")"
    cp -a "${source}" "${target}"
  fi
}

copy_tree_if_present() {
  local source="$1"
  local target="$2"
  if [ -d "${source}" ]; then
    mkdir -p "${target}"
    cp -a "${source}/." "${target}/"
  fi
}

copy_if_present "${VAPOR_CLIENT}/README.md" "${SITE}/client/README.md"
copy_if_present "${VAPOR_CLIENT}/App.vapor.toml" "${SITE}/client/App.vapor.toml"
copy_if_present "${VAPOR_CLIENT}/App-Source.vapor.toml" "${SITE}/client/App-Source.vapor.toml"
copy_tree_if_present "${VAPOR_CLIENT}/Vapor-CLI/crates/vapor_cli/docs" "${SITE}/shell"
copy_tree_if_present "${VAPOR_CLIENT}/Vapor/docs/roadmap" "${SITE}/vapor/roadmap"

BOOKS_ROOT="${VAPOR_CLIENT}/Vapor/docs/books"
MDBOOK_AVAILABLE=0
if command -v mdbook >/dev/null 2>&1 && mdbook --version >/dev/null 2>&1; then
  MDBOOK_AVAILABLE=1
fi
if [ -d "${BOOKS_ROOT}" ]; then
  install -d -m 0755 "${SITE}/books"
  while IFS= read -r -d '' book; do
    book_name="$(basename -- "$(dirname -- "${book}")")"
    if [ "${MDBOOK_AVAILABLE}" -eq 1 ]; then
      mdbook build "$(dirname -- "${book}")" --dest-dir "${SITE}/books/${book_name}"
    else
      copy_tree_if_present "$(dirname -- "${book}")/src" "${SITE}/books/${book_name}/src"
      copy_if_present "${book}" "${SITE}/books/${book_name}/book.toml"
    fi
  done < <(find "${BOOKS_ROOT}" -mindepth 2 -maxdepth 2 -name book.toml -print0 | sort -z)
fi

python3 - "${SITE}" "${BOOKS_ROOT}" "${VAPOR_CLIENT}/Vapor-CLI/crates/vapor_cli/docs" <<'PYINDEX'
from html import escape
from pathlib import Path
import sys
import tomllib
from urllib.parse import quote

site, books_root, guides_root = map(Path, sys.argv[1:])
books = []
if books_root.is_dir():
    for config_path in sorted(books_root.glob("*/book.toml")):
        metadata = tomllib.loads(config_path.read_text(encoding="utf-8")).get("book", {})
        name = config_path.parent.name
        built_index = site / "books" / name / "index.html"
        summary = site / "books" / name / "src" / "SUMMARY.md"
        target = built_index if built_index.is_file() else summary
        if target.is_file():
            books.append(
                (
                    metadata.get("title", name),
                    metadata.get("description", ""),
                    target.relative_to(site).as_posix(),
                )
            )

guides = []
if guides_root.is_dir():
    for guide in sorted(guides_root.rglob("*.md")):
        relative = guide.relative_to(guides_root).as_posix()
        lines = guide.read_text(encoding="utf-8").splitlines()
        title = next(
            (
                line.strip()[2:].strip()
                for line in lines
                if line.strip().startswith("# ") and line.strip()[2:].strip()
            ),
            guide.stem.replace("-", " ").replace("_", " "),
        )
        guides.append((title, "shell/" + quote(relative, safe="/")))

book_links = "\n".join(
    f'<li><a href="{escape(path, quote=True)}">{escape(title)}</a>{f"<p>{escape(description)}</p>" if description else ""}</li>'
    for title, description, path in books
)
guide_links = "\n".join(
    f'<li><a href="{escape(path, quote=True)}">{escape(title)}</a></li>' for title, path in guides
)
client_links = []
for relative, label in (
    ("client/README.md", "Vapor Client README"),
    ("client/App.vapor.toml", "Runtime manifest"),
    ("client/App-Source.vapor.toml", "Source manifest"),
    ("vapor/roadmap/README.md", "Roadmap"),
):
    if (site / relative).is_file():
        client_links.append(f'<li><a href="{escape(relative, quote=True)}">{escape(label)}</a></li>')

contents = f"""<!doctype html>
<meta charset="utf-8">
<title>Vapor Documentation</title>
<style>
body {{ max-width: 960px; margin: 3rem auto; padding: 0 1.5rem; font: 16px/1.5 system-ui, sans-serif; }}
code {{ background: #f4f4f4; padding: .1rem .25rem; border-radius: .2rem; }}
li {{ margin: .25rem 0; }}
</style>
<h1>Vapor Documentation</h1>
<p>This is the current deployed Vapor documentation bundle.</p>
<h2>Product books</h2>
<ul>{book_links}</ul>
<h2>Shell guides</h2>
<ul>{guide_links}</ul>
<h2>Client context</h2>
<ul>{''.join(client_links)}</ul>
"""
(site / "index.html").write_text(contents, encoding="utf-8")
PYINDEX


tar -C "${SITE}" -czf "${OUTPUT}" .
echo "${OUTPUT}"
