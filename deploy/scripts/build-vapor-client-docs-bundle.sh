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
if [ ! -f "${VAPOR_CLIENT}/Vapor-Standalone.toml" ]; then
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
copy_if_present "${VAPOR_CLIENT}/Vapor-Standalone.toml" "${SITE}/client/Vapor-Standalone.toml"
copy_tree_if_present "${VAPOR_CLIENT}/Vapor-CLI/crates/vapor_cli/docs" "${SITE}/shell"
copy_tree_if_present "${VAPOR_CLIENT}/Vapor/docs/roadmap" "${SITE}/vapor/roadmap"

BOOKS_ROOT="${VAPOR_CLIENT}/Vapor/docs/books"
MDBOOK_AVAILABLE=0
if command -v mdbook >/dev/null 2>&1 && mdbook --version >/dev/null 2>&1; then
  MDBOOK_AVAILABLE=1
fi
if [ -d "${BOOKS_ROOT}" ] && [ "${MDBOOK_AVAILABLE}" -ne 1 ]; then
  echo "error: mdbook is required to publish product books as readable HTML" >&2
  echo "Install mdbook before retrying; publishing raw Markdown as a fallback is disabled." >&2
  exit 1
fi
if [ -d "${BOOKS_ROOT}" ]; then
  install -d -m 0755 "${SITE}/books"
  while IFS= read -r -d '' book; do
    book_name="$(basename -- "$(dirname -- "${book}")")"
    mdbook build "$(dirname -- "${book}")" --dest-dir "${SITE}/books/${book_name}"
  done < <(find "${BOOKS_ROOT}" -mindepth 2 -maxdepth 2 -name book.toml -print0 | sort -z)
fi

python3 - "${SITE}" "${BOOKS_ROOT}" "${VAPOR_CLIENT}/Vapor-CLI/crates/vapor_cli/docs" <<'PYINDEX'
from html import escape
from pathlib import Path
import re
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
        if not built_index.is_file():
            raise SystemExit(f"book {name!r} did not produce an HTML index")
        books.append(
            (
                metadata.get("title", name),
                metadata.get("description", ""),
                built_index.relative_to(site).as_posix(),
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
        summary = ""
        for line in lines:
            text = line.strip()
            if not text or text.startswith("#") or text.startswith("```"):
                continue
            text = re.sub(r"!?(\[([^]]+)\])\([^)]+\)", r"\2", text)
            text = re.sub(r"[`*_~]", "", text)
            text = re.sub(r"\s+", " ", text).strip()
            if text:
                summary = text[:180] + ("…" if len(text) > 180 else "")
                break
        guides.append((title, summary, "shell/" + quote(relative, safe="/")))

book_links = "\n".join(
    f'<a class="card" href="{escape(path, quote=True)}"><strong>{escape(title)}</strong>{f"<p>{escape(description)}</p>" if description else ""}<span class="open-label">Open book</span></a>'
    for title, description, path in books
)
guide_links = "\n".join(
    f'<a class="card" href="{escape(path, quote=True)}"><strong>{escape(title)}</strong>{f"<p>{escape(summary)}</p>" if summary else ""}<span class="open-label">Open guide</span></a>' for title, summary, path in guides
)
client_links = []
for relative, label in (
    ("client/README.md", "Vapor Client README"),
    ("client/Vapor-Standalone.toml", "Standalone runtime manifest"),
    ("vapor/roadmap/README.md", "Roadmap"),
):
    if (site / relative).is_file():
        client_links.append(f'<a class="card" href="{escape(relative, quote=True)}"><strong>{escape(label)}</strong><p>Vapor runtime reference</p><span class="open-label">Open reference</span></a>')

publication_summary = f"{len(books)} books · {len(guides)} guides · {len(client_links)} references"

contents = f"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="theme-color" content="#10161d">
<meta name="description" content="Guides and references for the Vapor platform and its tools.">
<title>Documentation · Vapor</title>
<style>
:root {{ color-scheme: dark; font: 15px/1.5 Inter, ui-sans-serif, system-ui, sans-serif; background: #111714; color: #edf2ef; font-synthesis: none; }}
* {{ box-sizing: border-box; }}
body {{ margin: 0; min-height: 100vh; background: #111714; }}
a {{ color: #a6d4bd; }}
a:focus-visible {{ outline: 2px solid #b4dfc9; outline-offset: 3px; border-radius: 2px; }}
main {{ width: min(960px, calc(100% - 32px)); margin: 0 auto; padding: 20px 0 48px; }}
.topbar {{ display: flex; align-items: center; gap: 20px; padding-bottom: 44px; border-bottom: 1px solid #303a34; }}
.brand {{ margin-right: auto; color: #edf2ef; font-weight: 700; letter-spacing: .02em; text-decoration: none; }}
.topbar a:not(.brand) {{ color: #bdc8c0; text-decoration: none; font-size: .9rem; }}
.eyebrow {{ margin: 0; color: #a6d4bd; font-size: .78rem; font-weight: 650; }}
h1 {{ margin: 12px 0 12px; font-size: clamp(2.25rem, 6vw, 3.5rem); letter-spacing: -.045em; line-height: 1.02; }}
.lead {{ max-width: 680px; margin: 0; color: #c0cac3; font-size: 1rem; line-height: 1.6; }}
.freshness {{ margin: 12px 0 0; color: #9ba89f; font-size: .84rem; }}
.catalog {{ margin-top: 32px; }}
.catalog-head {{ display: flex; align-items: baseline; justify-content: space-between; gap: 16px; margin-bottom: 12px; }}
h2 {{ margin: 0; font-size: 1.06rem; letter-spacing: -.01em; }}
.catalog-head p {{ margin: 0; color: #9ba89f; font-size: .84rem; }}
.cards {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 8px; }}
.card {{ min-height: 112px; padding: 16px; border: 1px solid #36423a; border-radius: 4px; background: #171e19; text-decoration: none; transition: border-color .12s; }}
.card:hover {{ border-color: #7eaa91; }}
.card strong {{ display: block; color: #edf2ef; font-size: .96rem; }}
.card p {{ margin: 6px 0 0; color: #bac5bd; font-size: .86rem; line-height: 1.5; }}
.card .open-label {{ display: inline-block; margin-top: 12px; color: #a6d4bd; font-size: .8rem; font-weight: 650; }}
.empty {{ margin: 0; padding: 16px; border: 1px solid #36423a; border-radius: 4px; color: #aeb9b1; }}
footer {{ display: flex; flex-wrap: wrap; gap: 16px; margin-top: 40px; padding-top: 16px; border-top: 1px solid #303a34; color: #9ba89f; font-size: .84rem; }}
footer span {{ margin-right: auto; }}
@media (max-width: 620px) {{ .topbar {{ gap: 14px; padding-bottom: 28px; }} .catalog-head {{ align-items: start; flex-direction: column; gap: 4px; }} .card {{ min-height: 0; }} }}
@media (prefers-reduced-motion: reduce) {{ *, *::before, *::after {{ transition-duration: .01ms !important; scroll-behavior: auto !important; }} }}
</style>
</head>
<body>
<main>
<nav class="topbar" aria-label="Main navigation"><a class="brand" href="/">VAPOR</a><a href="/downloads">Get Vapor</a><a href="/license">License</a></nav>
<header><p class="eyebrow">Vapor library</p><h1>Documentation</h1><p class="lead">Books, guides, and reference material for working with Vapor and its tools.</p><p class="freshness">Current bundle · {publication_summary}</p></header>
<section class="catalog" aria-labelledby="books-title"><div class="catalog-head"><h2 id="books-title">Product books</h2><p>Guides organized by product area</p></div><div class="cards">{book_links or '<p class="empty">No product books are published yet.</p>'}</div></section>
<section class="catalog" aria-labelledby="guides-title"><div class="catalog-head"><h2 id="guides-title">Shell guides</h2><p>Commands and runtime references</p></div><div class="cards">{guide_links or '<p class="empty">No shell guides are published yet.</p>'}</div></section>
<section class="catalog" aria-labelledby="reference-title"><div class="catalog-head"><h2 id="reference-title">Runtime references</h2><p>Manifests and project context</p></div><div class="cards">{''.join(client_links) or '<p class="empty">No runtime references are published yet.</p>'}</div></section>
<footer><span>Vapor documentation</span><a href="/">Home</a><a href="/downloads">Get Vapor</a><a href="/login">Sign in</a></footer>
</main>
</body>
</html>
"""
(site / "index.html").write_text(contents, encoding="utf-8")
PYINDEX


tar -C "${SITE}" -czf "${OUTPUT}" .
echo "${OUTPUT}"
