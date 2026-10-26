#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "${SCRIPT_DIR}/lib.sh"

require_root

ATLAS_REPO="${VAPOR_DEPLOY_ROOT}/Atlas"
RELEASES_DIR="${VAPOR_ATLAS_WEB_ROOT}/releases"
CURRENT_LINK="${VAPOR_ATLAS_WEB_ROOT}/current"

if [ ! -d "${ATLAS_REPO}/.git" ] && [ ! -f "${ATLAS_REPO}/.git" ]; then
  echo "error: Atlas submodule is missing at ${ATLAS_REPO}" >&2
  exit 1
fi

if ! git check-ref-format --branch "${VAPOR_ATLAS_REF}" >/dev/null 2>&1; then
  echo "error: invalid VAPOR_ATLAS_REF: ${VAPOR_ATLAS_REF}" >&2
  exit 1
fi

install -d -o root -g root -m 0755 "${VAPOR_ATLAS_WEB_ROOT}"
install -d -o root -g root -m 0755 "${RELEASES_DIR}"
git -C "${ATLAS_REPO}" fetch --quiet origin "${VAPOR_ATLAS_REF}"
SOURCE_REVISION="$(git -C "${ATLAS_REPO}" rev-parse --verify 'FETCH_HEAD^{commit}')"
git -C "${ATLAS_REPO}" checkout --quiet --detach "${SOURCE_REVISION}"

if [ -L "${CURRENT_LINK}" ] && [ "$(basename -- "$(readlink -- "${CURRENT_LINK}")")" = "${SOURCE_REVISION}" ]; then
  echo "atlas: ${SOURCE_REVISION} is already published"
  exit 0
fi

RELEASE_DIR="${RELEASES_DIR}/${SOURCE_REVISION}"
if [ ! -d "${RELEASE_DIR}" ]; then
  STAGING_DIR="$(mktemp -d "${VAPOR_ATLAS_WEB_ROOT}/.staging.XXXXXXXX")"
  trap 'rm -rf -- "${STAGING_DIR}"' EXIT

  python3 "${ATLAS_REPO}/tools/atlas.py" validate --root "${ATLAS_REPO}"
  python3 "${ATLAS_REPO}/tools/atlas.py" build \
    --root "${ATLAS_REPO}" \
    --output "${STAGING_DIR}" \
    --base-url /atlas/ \
    --source-revision "${SOURCE_REVISION}"

  find "${STAGING_DIR}" -type d -exec chmod 0755 {} +
  find "${STAGING_DIR}" -type f -exec chmod 0644 {} +
  mv -- "${STAGING_DIR}" "${RELEASE_DIR}"
  trap - EXIT
fi

PREVIOUS_REVISION=""
if [ -L "${CURRENT_LINK}" ]; then
  PREVIOUS_REVISION="$(basename -- "$(readlink -- "${CURRENT_LINK}")")"
  PREVIOUS_NEXT_LINK="${VAPOR_ATLAS_WEB_ROOT}/previous.next"
  rm -f -- "${PREVIOUS_NEXT_LINK}"
  ln -s "releases/${PREVIOUS_REVISION}" "${PREVIOUS_NEXT_LINK}"
  mv -Tf -- "${PREVIOUS_NEXT_LINK}" "${VAPOR_ATLAS_WEB_ROOT}/previous"
fi

NEXT_LINK="${VAPOR_ATLAS_WEB_ROOT}/current.next"
rm -f -- "${NEXT_LINK}"
ln -s "releases/${SOURCE_REVISION}" "${NEXT_LINK}"
mv -Tf -- "${NEXT_LINK}" "${CURRENT_LINK}"

# Keep one previous build for a quick operator rollback without letting
# generated site releases accumulate indefinitely.
for OLD_RELEASE in "${RELEASES_DIR}"/*; do
  [ -d "${OLD_RELEASE}" ] || continue
  OLD_REVISION="$(basename -- "${OLD_RELEASE}")"
  if [ "${OLD_REVISION}" != "${SOURCE_REVISION}" ] && [ "${OLD_REVISION}" != "${PREVIOUS_REVISION}" ]; then
    rm -rf -- "${OLD_RELEASE}"
  fi
done

echo "atlas: published ${SOURCE_REVISION} at ${VAPOR_ATLAS_WEB_ROOT}/current"
