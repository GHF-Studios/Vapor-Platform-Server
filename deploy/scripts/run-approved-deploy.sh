#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "${SCRIPT_DIR}/lib.sh"

require_root
QUEUE_DB="${VAPOR_PLATFORM_DB:-${VAPOR_PLATFORM_STATE:-${VAPOR_STATE_ROOT}/platform}/platform.sqlite3}"
if [ "$#" -ne 1 ] || [[ ! "$1" =~ ^[0-9a-fA-F-]{36}$ ]]; then
  echo "deploy-worker: one approved request UUID is required" >&2
  exit 2
fi
REQUEST_ID="$1"
CLAIM="$(python3 "${SCRIPT_DIR}/deploy-request-queue.py" --database "${QUEUE_DB}" claim --id "${REQUEST_ID}")"
if [ "${CLAIM}" = "null" ]; then
  echo "deploy-worker: no approved request is waiting"
  exit 0
fi

REQUEST_ID="$(python3 -c 'import json,sys; print(json.loads(sys.argv[1])["id"])' "${CLAIM}")"
REQUEST_VERSION="$(python3 -c 'import json,sys; print(json.loads(sys.argv[1])["version"])' "${CLAIM}")"
REQUEST_COMMIT="$(python3 -c 'import json,sys; print(json.loads(sys.argv[1])["request"]["commit"])' "${CLAIM}")"
REQUEST_TREE="$(python3 -c 'import json,sys; print(json.loads(sys.argv[1])["request"]["tree"])' "${CLAIM}")"
REQUEST_MODE="$(python3 -c 'import json,sys; print(json.loads(sys.argv[1])["request"]["server_mode"])' "${CLAIM}")"

report_result() {
  local result="$1"
  local detail="$2"
  python3 "${SCRIPT_DIR}/deploy-request-queue.py" \
    --database "${QUEUE_DB}" complete \
    --id "${REQUEST_ID}" \
    --version "${REQUEST_VERSION}" \
    --result "${result}" \
    --detail "${detail}"
}

fail_claim() {
  local message="$1"
  echo "deploy-worker: ${message}" >&2
  report_result failed "${message}" || true
  exit 1
}

if [[ ! "${REQUEST_COMMIT}" =~ ^[A-Fa-f0-9]{40}$ || ! "${REQUEST_TREE}" =~ ^[A-Fa-f0-9]{40}$ ]]; then
  fail_claim "request contained an invalid root revision"
fi
if [[ "${REQUEST_MODE}" != "monolith" && "${REQUEST_MODE}" != "services" ]]; then
  fail_claim "request contained an invalid deployment target"
fi
if [ ! -d "${VAPOR_DEPLOY_ROOT}/.git" ]; then
  fail_claim "the bootstrapped Platform source checkout is missing"
fi
if [ -n "$(git -C "${VAPOR_DEPLOY_ROOT}" status --porcelain)" ]; then
  fail_claim "the Platform source checkout has local changes"
fi
ORIGIN_URL="$(git -C "${VAPOR_DEPLOY_ROOT}" remote get-url origin)"
if [ "${ORIGIN_URL}" != "https://github.com/GHF-Studios/Vapor-Platform-Server.git" ]; then
  fail_claim "the Platform checkout origin does not match the approved repository"
fi
if [ "${VAPOR_BRANCH}" != "main" ]; then
  fail_claim "the configured deployment branch is not the approved main branch"
fi

if ! git -C "${VAPOR_DEPLOY_ROOT}" fetch --no-tags origin "+refs/heads/main:refs/remotes/origin/main"; then
  fail_claim "could not fetch the protected deployment branch"
fi
if ! git -C "${VAPOR_DEPLOY_ROOT}" cat-file -e "${REQUEST_COMMIT}^{commit}"; then
  fail_claim "the approved commit is absent from the fetched repository"
fi
if ! git -C "${VAPOR_DEPLOY_ROOT}" merge-base --is-ancestor "${REQUEST_COMMIT}" "origin/${VAPOR_BRANCH}"; then
  fail_claim "the approved commit is no longer reachable from the protected branch"
fi
ACTUAL_TREE="$(git -C "${VAPOR_DEPLOY_ROOT}" rev-parse "${REQUEST_COMMIT}^{tree}")"
if [ "${ACTUAL_TREE}" != "${REQUEST_TREE}" ]; then
  fail_claim "the approved root tree does not match its request"
fi

PINS_JSON="$(python3 -c 'import json,sys; print(json.dumps(json.loads(sys.argv[1])["request"]["submodules"], separators=(",", ":")))' "${CLAIM}")"
if ! python3 "${SCRIPT_DIR}/verify-deploy-source.py" \
  --root "${VAPOR_DEPLOY_ROOT}" \
  --commit "${REQUEST_COMMIT}" \
  --tree "${REQUEST_TREE}" \
  --pins-json "${PINS_JSON}" >/dev/null; then
  fail_claim "the approved root tree, submodule pins, or first-party source URLs are invalid"
fi

if ! git -C "${VAPOR_DEPLOY_ROOT}" checkout --detach --force "${REQUEST_COMMIT}"; then
  fail_claim "could not check out the exact approved root commit"
fi
git -C "${VAPOR_DEPLOY_ROOT}" submodule sync
if ! git -C "${VAPOR_DEPLOY_ROOT}" submodule update --init; then
  fail_claim "could not check out the approved service submodules"
fi
for path in $(python3 - "${CLAIM}" <<'PY'
import json, sys
for item in json.loads(sys.argv[1])["request"]["submodules"]:
    print(item["path"])
PY
); do
  expected="$(python3 - "${CLAIM}" "${path}" <<'PY'
import json, sys
pins = json.loads(sys.argv[1])["request"]["submodules"]
print(next(item["commit"] for item in pins if item["path"] == sys.argv[2]))
PY
)"
  actual="$(git -C "${VAPOR_DEPLOY_ROOT}/${path}" rev-parse HEAD)"
  if [ "${actual}" != "${expected}" ]; then
    fail_claim "submodule ${path} is not at its approved commit"
  fi
done

set +e
VAPOR_APPROVED_DEPLOY_ID="${REQUEST_ID}" \
VAPOR_APPROVED_DEPLOY_VERSION="${REQUEST_VERSION}" \
VAPOR_APPROVED_REVISION="${REQUEST_COMMIT}" \
VAPOR_SERVER_MODE="${REQUEST_MODE}" \
"${VAPOR_DEPLOY_ROOT}/deploy/scripts/deploy.sh" --approved "${REQUEST_COMMIT}"
DEPLOY_STATUS=$?
set -e

if [ "${DEPLOY_STATUS}" -eq 0 ]; then
  report_result succeeded "approved commit ${REQUEST_COMMIT} deployed"
else
  report_result failed "approved commit ${REQUEST_COMMIT} deployment failed with status ${DEPLOY_STATUS}"
fi
exit "${DEPLOY_STATUS}"
