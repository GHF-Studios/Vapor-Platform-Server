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
copy_tree_if_present "${VAPOR_CLIENT}/Vapor-Shell/crates/vapor_shell/docs" "${SITE}/shell"
copy_tree_if_present "${VAPOR_CLIENT}/Vapor/docs/roadmap" "${SITE}/vapor/roadmap"

BOOKS_ROOT="${VAPOR_CLIENT}/Vapor/docs/books"
if [ -d "${BOOKS_ROOT}" ]; then
  install -d -m 0755 "${SITE}/books"
  while IFS= read -r -d '' book; do
    book_name="$(basename -- "$(dirname -- "${book}")")"
    if command -v mdbook >/dev/null 2>&1; then
      mdbook build "$(dirname -- "${book}")" --dest-dir "${SITE}/books/${book_name}"
    else
      copy_tree_if_present "$(dirname -- "${book}")/src" "${SITE}/books/${book_name}/src"
      copy_if_present "${book}" "${SITE}/books/${book_name}/book.toml"
    fi
  done < <(find "${BOOKS_ROOT}" -mindepth 2 -maxdepth 2 -name book.toml -print0 | sort -z)
fi

cat > "${SITE}/index.html" <<'HTML'
<!doctype html>
<meta charset="utf-8">
<title>Vapor Documentation</title>
<style>
body { max-width: 960px; margin: 3rem auto; padding: 0 1.5rem; font: 16px/1.5 system-ui, sans-serif; }
code { background: #f4f4f4; padding: .1rem .25rem; border-radius: .2rem; }
li { margin: .25rem 0; }
</style>
<h1>Vapor Documentation</h1>
<p>This is the current deployed Vapor documentation bundle.</p>
<h2>Product books</h2>
<ul>
  <li><a href="books/index/index.html">Docs index</a></li>
  <li><a href="books/vapor/index.html">Vapor Book</a></li>
  <li><a href="books/sdk/index.html">SDK Book</a></li>
  <li><a href="books/launcher/index.html">Launcher Book</a></li>
  <li><a href="books/distribution/index.html">Distribution Book</a></li>
  <li><a href="books/internals/index.html">Internals Book</a></li>
  <li><a href="books/cookbook/index.html">Cookbook</a></li>
</ul>
<h2>Shell docs</h2>
<ul>
  <li><a href="shell/commands.md">Commands</a></li>
  <li><a href="shell/setup.md">Setup</a></li>
  <li><a href="shell/distribution.md">Distribution</a></li>
  <li><a href="shell/steam-development.md">Steam development</a></li>
  <li><a href="shell/discovery.md">Discovery</a></li>
</ul>
<h2>Client context</h2>
<ul>
  <li><a href="client/README.md">Vapor Client README</a></li>
  <li><a href="client/App.vapor.toml">Runtime manifest</a></li>
  <li><a href="client/App-Source.vapor.toml">Source manifest</a></li>
  <li><a href="vapor/roadmap/README.md">Roadmap</a></li>
</ul>
HTML

tar -C "${SITE}" -czf "${OUTPUT}" .
echo "${OUTPUT}"
