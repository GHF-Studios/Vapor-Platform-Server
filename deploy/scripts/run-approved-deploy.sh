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
REQUEST_SCOPE="$(python3 -c 'import json,sys; print(json.loads(sys.argv[1])["request"]["deployment_scope"])' "${CLAIM}")"
REQUEST_TARGET="$(python3 -c 'import json,sys; print(json.loads(sys.argv[1])["request"].get("service_target") or "")' "${CLAIM}")"
REQUEST_ROOT="${VAPOR_DEPLOY_ROOT}"
SCOPED_WORKTREE=""
CLEANUP_WARNING=""

remove_scoped_checkout() {
  local status=$?
  trap - EXIT
  if [ -n "${SCOPED_WORKTREE}" ]; then
    if [[ "${SCOPED_WORKTREE}" != "${VAPOR_DEPLOY_ROOT}.scoped."* ]] || \
      [ "${SCOPED_WORKTREE}" = "${VAPOR_DEPLOY_ROOT}" ] || [ -L "${SCOPED_WORKTREE}" ] || \
      ! rm -rf -- "${SCOPED_WORKTREE}"; then
      echo "deploy-worker: warning: isolated temporary source cleanup failed at ${SCOPED_WORKTREE}" >&2
      status=1
    fi
  fi
  exit "${status}"
}

report_result() {
  local result="$1"
  local detail="$2"
  python3 "${SCRIPT_DIR}/deploy-request-queue.py" \
    --database "${QUEUE_DB}" complete \
    --id "${REQUEST_ID}" \
    --version "${REQUEST_VERSION}" \
    --result "${result}" \
    --detail "${detail}"
  local next_id
  next_id="$(python3 "${SCRIPT_DIR}/deploy-request-queue.py" --database "${QUEUE_DB}" next)"
  if [[ "${next_id}" =~ ^[0-9a-fA-F-]{36}$ ]]; then
    if ! systemctl start --no-block "vapor-deploy@${next_id}.service"; then
      echo "deploy-worker: could not wake the next approved request ${next_id}; it remains queued for retry" >&2
    fi
  fi
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
if [[ "${REQUEST_SCOPE}" != "full" && "${REQUEST_SCOPE}" != "service" ]]; then
  fail_claim "request contained an invalid deployment scope"
fi
if [[ "${REQUEST_SCOPE}" = "full" && -n "${REQUEST_TARGET}" ]]; then
  fail_claim "full deployment request unexpectedly named a service target"
fi
if [[ "${REQUEST_SCOPE}" = "service" && -z "${REQUEST_TARGET}" ]]; then
  fail_claim "service deployment request has no service target"
fi
if [[ "${REQUEST_SCOPE}" = "service" && "${REQUEST_MODE}" != "services" ]]; then
  fail_claim "service deployment request requires services mode"
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
LATEST_MAIN_COMMIT="$(git -C "${VAPOR_DEPLOY_ROOT}" rev-parse "origin/${VAPOR_BRANCH}")"
if [ "${REQUEST_COMMIT}" != "${LATEST_MAIN_COMMIT}" ]; then
  fail_claim "this approved revision was superseded by a newer main commit; no older snapshot was deployed"
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

if [ "${REQUEST_SCOPE}" = "full" ]; then
  if ! git -C "${VAPOR_DEPLOY_ROOT}" checkout --detach --force "${REQUEST_COMMIT}"; then
    fail_claim "could not check out the exact approved root commit"
  fi
  git -C "${VAPOR_DEPLOY_ROOT}" submodule sync
  if ! git -C "${VAPOR_DEPLOY_ROOT}" submodule update --init; then
    fail_claim "could not check out the approved service submodules"
  fi
else
  SCOPED_WORKTREE="$(mktemp -d "${VAPOR_DEPLOY_ROOT}.scoped.XXXXXXXX")"
  trap remove_scoped_checkout EXIT
  # Worktrees with submodules have incomplete Git support; a private clone
  # gives the temporary checkout its own .git/modules tree and cannot retarget
  # submodule metadata used by the canonical deployment checkout.
  if ! git clone --no-checkout --no-hardlinks "${VAPOR_DEPLOY_ROOT}" "${SCOPED_WORKTREE}"; then
    fail_claim "could not create an isolated temporary source clone"
  fi
  REQUEST_ROOT="${SCOPED_WORKTREE}"
  if ! git -C "${REQUEST_ROOT}" checkout --detach --force "${REQUEST_COMMIT}"; then
    fail_claim "could not check out the exact approved root commit in the temporary clone"
  fi
  REQUEST_SUBMODULE_PATH="$(python3 - "${REQUEST_ROOT}" "${REQUEST_TARGET}" <<'PY'
import sys
sys.path.insert(0, sys.argv[1] + "/deploy/scripts")
from service_deploy_map import target_map
print(target_map(sys.argv[2])["package_path"])
PY
)"
  git -C "${REQUEST_ROOT}" submodule sync -- "${REQUEST_SUBMODULE_PATH}"
  if ! git -C "${REQUEST_ROOT}" submodule update --init -- "${REQUEST_SUBMODULE_PATH}"; then
    fail_claim "could not check out the approved target submodule in the isolated clone"
  fi
fi
if ! python3 "${REQUEST_ROOT}/deploy/scripts/verify-deploy-source.py" \
  --root "${REQUEST_ROOT}" \
  --commit "${REQUEST_COMMIT}" \
  --tree "${REQUEST_TREE}" \
  --pins-json "${PINS_JSON}" >/dev/null; then
  fail_claim "the checked-out root tree, submodule pins, or first-party source URLs are invalid"
fi
for path in $(python3 - "${CLAIM}" "${REQUEST_SUBMODULE_PATH:-}" <<'PY'
import json, sys
scope = json.loads(sys.argv[1])["request"]["deployment_scope"]
target_path = sys.argv[2]
for item in json.loads(sys.argv[1])["request"]["submodules"]:
    if scope == "full" or item["path"] == target_path:
        print(item["path"])
PY
); do
  expected="$(python3 - "${CLAIM}" "${path}" <<'PY'
import json, sys
pins = json.loads(sys.argv[1])["request"]["submodules"]
print(next(item["commit"] for item in pins if item["path"] == sys.argv[2]))
PY
)"
  actual="$(git -C "${REQUEST_ROOT}/${path}" rev-parse HEAD)"
  if [ "${actual}" != "${expected}" ]; then
    fail_claim "submodule ${path} is not at its approved commit"
  fi
done

set +e
if [ "${REQUEST_SCOPE}" = "full" ]; then
  VAPOR_APPROVED_DEPLOY_ID="${REQUEST_ID}" \
  VAPOR_APPROVED_DEPLOY_VERSION="${REQUEST_VERSION}" \
  VAPOR_APPROVED_REVISION="${REQUEST_COMMIT}" \
  VAPOR_SERVER_MODE="${REQUEST_MODE}" \
  VAPOR_DEPLOYMENT_SCOPE="${REQUEST_SCOPE}" \
  VAPOR_SERVICE_TARGET="${REQUEST_TARGET}" \
  "${REQUEST_ROOT}/deploy/scripts/deploy.sh" --approved "${REQUEST_COMMIT}"
  DEPLOY_STATUS=$?
else
  VAPOR_APPROVED_DEPLOY_ID="${REQUEST_ID}" \
  VAPOR_APPROVED_DEPLOY_VERSION="${REQUEST_VERSION}" \
  VAPOR_APPROVED_REVISION="${REQUEST_COMMIT}" \
  VAPOR_SERVER_MODE="${REQUEST_MODE}" \
  VAPOR_DEPLOYMENT_SCOPE="${REQUEST_SCOPE}" \
  VAPOR_SERVICE_TARGET="${REQUEST_TARGET}" \
  VAPOR_APPROVED_SOURCE_ROOT="${REQUEST_ROOT}" \
  VAPOR_APPROVED_TREE="${REQUEST_TREE}" \
  python3 "${REQUEST_ROOT}/deploy/scripts/deploy-service.py" --approved "${REQUEST_COMMIT}" --target "${REQUEST_TARGET}"
  DEPLOY_STATUS=$?
fi
set -e

if [ -n "${SCOPED_WORKTREE}" ]; then
  if [[ "${SCOPED_WORKTREE}" == "${VAPOR_DEPLOY_ROOT}.scoped."* ]] && \
    [ "${SCOPED_WORKTREE}" != "${VAPOR_DEPLOY_ROOT}" ] && [ ! -L "${SCOPED_WORKTREE}" ] && \
    rm -rf -- "${SCOPED_WORKTREE}"; then
    SCOPED_WORKTREE=""
  else
    CLEANUP_WARNING="; isolated temporary source cleanup failed at ${SCOPED_WORKTREE}"
  fi
  trap - EXIT
fi

if [ "${DEPLOY_STATUS}" -eq 0 ]; then
  report_result succeeded "approved ${REQUEST_SCOPE} deployment ${REQUEST_TARGET:-all} for ${REQUEST_COMMIT} completed${CLEANUP_WARNING}"
else
  case "${DEPLOY_STATUS}" in
    2) failure_detail="service deployment ${REQUEST_TARGET} failed; previous executable and service state were restored" ;;
    3) failure_detail="service deployment ${REQUEST_TARGET} failed and rollback is incomplete; inspect preserved recovery material" ;;
    *) failure_detail="approved ${REQUEST_SCOPE} deployment ${REQUEST_TARGET:-all} for ${REQUEST_COMMIT} failed with status ${DEPLOY_STATUS}" ;;
  esac
  report_result failed "${failure_detail}${CLEANUP_WARNING}"
fi
exit "${DEPLOY_STATUS}"
