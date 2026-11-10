#!/usr/bin/env bash
set -Eeuo pipefail
umask 077

ROOT=/opt/vapor-platform-server
REPO_URL=https://github.com/GHF-Studios/Vapor-Platform-Server.git
STATE_ROOT=/var/lib/vapor-server
CONFIG_ROOT=/etc/vapor-server
BACKUP_ROOT=/var/backups/vapor-server
REPORT_ID="${1:-}"

if [ "$(id -u)" -ne 0 ]; then
  echo 'Run this migration with sudo.' >&2
  exit 1
fi
if [ "$#" -ne 1 ] || [[ ! "$REPORT_ID" =~ ^[a-f0-9]{16}$ ]]; then
  echo 'Provide a one-time 16-character hexadecimal report code.' >&2
  exit 1
fi
if ! command -v flock >/dev/null 2>&1; then
  echo 'error: flock is required; no host changes made' >&2
  exit 1
fi

STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
CANDIDATE="/opt/vapor-platform-server.candidate-${STAMP}"
LEGACY="/opt/vapor-platform-server.legacy-${STAMP}"
FAILED="/opt/vapor-platform-server.failed-${STAMP}"
BACKUP="${BACKUP_ROOT}/cutover-${STAMP}"
STATE_BUNDLE="${BACKUP}/state.tar.gz"
REPORT="/tmp/vapor-cutover-${REPORT_ID}.txt"
REPORT_RESULT=not_started
REPORT_DETAIL=none
APPLYING=0
MIGRATION_SUCCEEDED=0
TIMER_WAS_ACTIVE=0
OLD_ROOT_MOVED=0
NEW_ROOT_INSTALLED=0
STATE_BACKED_UP=0
TEMP_SSH_GROUP_EXISTED=0
declare -A UNIT_ACTIVE UNIT_ENABLED UNIT_FILE_PRESENT
UNITS=(
  caddy.service
  vapor-platform-server.service
  vapor-platform-ops.socket
  vapor-homepage.service
  vapor-docs.service
  vapor-identity.service
  vapor-diagnostics.service
  vapor-registry.service
  vapor-artifact.service
  vapor-build-cache.service
  vapor-deploy.service
  vapor-deploy.timer
  vapor-deploy@.service
  vapor-state-export.service
  vapor-state-export.timer
)
TEMPLATE_UNITS=(vapor-platform-ops@.service)

mkdir -p -- "${BACKUP_ROOT}"
chmod 0700 -- "${BACKUP_ROOT}"
install -m 0600 /dev/null "${REPORT}"

report_line() { printf '%s=%s\n' "$1" "$2" >>"${REPORT}"; }
fail() {
  REPORT_RESULT=preflight_failed
  REPORT_DETAIL="$1"
  echo "error: $1; aborting" >&2
  exit 1
}

capture_unit_state() {
  local unit="$1" enabled
  if systemctl is-active --quiet "${unit}"; then UNIT_ACTIVE["${unit}"]=active; else UNIT_ACTIVE["${unit}"]=inactive; fi
  enabled="$(systemctl is-enabled "${unit}" 2>/dev/null || true)"
  UNIT_ENABLED["${unit}"]="${enabled:-disabled}"
  if [ -f "/etc/systemd/system/${unit}" ] && [ ! -L "/etc/systemd/system/${unit}" ]; then
    UNIT_FILE_PRESENT["${unit}"]=yes
  else
    UNIT_FILE_PRESENT["${unit}"]=no
  fi
}

safe_origin_repository() {
  python3 - "$1" <<'PY'
import re, sys
from urllib.parse import urlsplit

value = sys.argv[1]
if "://" in value:
    parsed = urlsplit(value)
    host, path = parsed.hostname or "", parsed.path.lstrip("/")
else:
    match = re.fullmatch(r"(?:[^@]+@)?([^:]+):(.+)", value)
    host, path = (match.group(1), match.group(2)) if match else ("", "")
path = path.removesuffix(".git")
if host.lower() == "github.com" and re.fullmatch(r"[A-Za-z0-9._/-]+", path):
    print(path)
else:
    print("unrecognized")
PY
}

check_ci_success() {
  local sha="$1" api runs
  api="https://api.github.com/repos/GHF-Studios/Vapor-Platform-Server/actions/runs?head_sha=${sha}&event=push&per_page=20"
  runs="$(curl --fail --silent --show-error --connect-timeout 5 --max-time 20 \
    -H 'Accept: application/vnd.github+json' -H 'User-Agent: vapor-platform-cutover' "$api")" || return 1
  printf '%s' "$runs" | python3 -c '
import json, sys, urllib.request
sha = sys.argv[1]
data = json.load(sys.stdin)
matches = [item for item in data.get("workflow_runs", [])
           if item.get("head_sha") == sha and item.get("event") == "push"
           and item.get("path", "").endswith("/deploy.yml")]
if not matches:
    raise SystemExit(1)
run = matches[0]
if run.get("status") != "completed" or run.get("conclusion") != "success":
    raise SystemExit(1)
request = urllib.request.Request(run["jobs_url"], headers={
    "Accept": "application/vnd.github+json", "User-Agent": "vapor-platform-cutover"})
with urllib.request.urlopen(request, timeout=20) as response:
    jobs = json.load(response).get("jobs", [])
if not any("root-ci" in job.get("name", "") and job.get("conclusion") == "success"
           for job in jobs):
    raise SystemExit(1)
' "$sha"
}

restore_unit_files() {
  local unit path active enabled restore_failed=0
  for unit in "${UNITS[@]}" "${TEMPLATE_UNITS[@]}"; do
    path="/etc/systemd/system/${unit}"
    rm -f -- "${path}" || restore_failed=1
    if [ "${UNIT_FILE_PRESENT[${unit}]:-no}" = yes ]; then
      cp -a -- "${BACKUP}/systemd/${unit}" "${path}" || restore_failed=1
    fi
  done
  # The retired polling deploy timer is intentionally never restored.
  rm -f -- /etc/systemd/system/vapor-deploy.timer /etc/systemd/system/vapor-deploy.service
  systemctl daemon-reload || restore_failed=1
  for unit in "${UNITS[@]}"; do
    case "${unit}" in
      vapor-deploy.service|vapor-deploy.timer|vapor-deploy@.service) continue ;;
    esac
    if [ "${UNIT_ENABLED[${unit}]:-not-found}" = not-found ]; then continue; fi
    case "${UNIT_ENABLED[${unit}]:-disabled}" in
      enabled|enabled-runtime) systemctl enable "${unit}" >/dev/null 2>&1 || restore_failed=1 ;;
      disabled) systemctl disable "${unit}" >/dev/null 2>&1 || restore_failed=1 ;;
    esac
    if [ "${UNIT_ACTIVE[${unit}]:-inactive}" = active ]; then
      systemctl start "${unit}" >/dev/null 2>&1 || restore_failed=1
      systemctl is-active --quiet "${unit}" || restore_failed=1
    else
      systemctl stop "${unit}" >/dev/null 2>&1 || restore_failed=1
      if systemctl is-active --quiet "${unit}"; then restore_failed=1; fi
    fi
  done
  return "${restore_failed}"
}

rollback_cutover() {
  local unit path rollback_failed=0
  set +e
  report_line rollback_started yes
  for unit in "${UNITS[@]}"; do systemctl stop "${unit}" >/dev/null 2>&1; done

  # Restore SSH policy from the saved snapshot. The uninstall helper refuses
  # active grants and is not safe to call as part of a rollback.
  rm -f -- /etc/vapor-server/ssh-access-enabled
  rm -f -- /etc/sudoers.d/vapor-platform-ops /etc/sudoers.d/vapor-temporary-access
  rm -f -- /usr/local/libexec/vapor-platform-ops
  rm -f -- /usr/local/sbin/vapor-ssh-deploy /usr/local/sbin/vapor-ssh-service-control
  for path in /etc/sudoers.d/vapor-platform-ops /etc/sudoers.d/vapor-temporary-access; do
    if [ -f "${BACKUP}/sudoers/$(basename -- "$path")" ]; then
      cp -a -- "${BACKUP}/sudoers/$(basename -- "$path")" "$path"
    fi
  done
  for path in /usr/local/libexec/vapor-platform-ops /usr/local/sbin/vapor-ssh-deploy /usr/local/sbin/vapor-ssh-service-control; do
    if [ -f "${BACKUP}/local/$(basename -- "$path")" ]; then
      cp -a -- "${BACKUP}/local/$(basename -- "$path")" "$path"
    fi
  done
  if [ "${TEMP_SSH_GROUP_EXISTED}" = 0 ] && getent group vapor-temp-ssh >/dev/null; then
    if getent group vapor-temp-ssh | awk -F: '{ exit ($4 == "") ? 0 : 1 }'; then
      groupdel vapor-temp-ssh || rollback_failed=1
    else
      report_line rollback_temp_ssh_group preserved_with_members
      rollback_failed=1
    fi
  fi

  if [ -d "${CONFIG_ROOT}" ] && [ -d "${BACKUP}/vapor-server-config" ]; then
    if mv -- "${CONFIG_ROOT}" "${BACKUP}/vapor-server-config.failed" 2>/dev/null; then
      cp -a -- "${BACKUP}/vapor-server-config" "${CONFIG_ROOT}" || rollback_failed=1
    else
      rollback_failed=1
    fi
  else
    rollback_failed=1
  fi
  if [ -d /etc/caddy ] && [ -d "${BACKUP}/caddy-config" ]; then
    if mv -- /etc/caddy "${BACKUP}/caddy-config.failed" 2>/dev/null; then
      cp -a -- "${BACKUP}/caddy-config" /etc/caddy || rollback_failed=1
    else
      rollback_failed=1
    fi
  else
    rollback_failed=1
  fi
  if [ -f "${BACKUP}/sshd_config" ]; then
    cp -a -- "${BACKUP}/sshd_config" /etc/ssh/sshd_config || rollback_failed=1
    sshd -t || rollback_failed=1
  else
    rollback_failed=1
  fi
  if [ -d "${BACKUP}/atlas-web" ]; then
    rm -rf -- /srv/vapor-web/atlas || rollback_failed=1
    mkdir -p -- /srv/vapor-web || rollback_failed=1
    cp -a -- "${BACKUP}/atlas-web" /srv/vapor-web/atlas || rollback_failed=1
  else
    rm -rf -- /srv/vapor-web/atlas || rollback_failed=1
  fi

  if [ "${STATE_BACKED_UP}" = 1 ] && [ -f "${STATE_BUNDLE}" ]; then
    local restore_tmp="${BACKUP}/restore-state"
    mkdir -p -- "${restore_tmp}"
    tar -xzf "${STATE_BUNDLE}" -C "${restore_tmp}"
    if [ -d "${restore_tmp}/vapor-server-state/state" ]; then
      if [ -d "${STATE_ROOT}" ]; then
        mv -- "${STATE_ROOT}" "${BACKUP}/state-after-failed-cutover" || rollback_failed=1
      fi
      mkdir -p -- "${STATE_ROOT}" || rollback_failed=1
      cp -a -- "${restore_tmp}/vapor-server-state/state/." "${STATE_ROOT}/" || rollback_failed=1
    else
      report_line rollback_state_restore failed
      rollback_failed=1
    fi
  fi

  if [ "${NEW_ROOT_INSTALLED}" = 1 ] && [ -d "${ROOT}" ]; then
    mv -- "${ROOT}" "${FAILED}" 2>/dev/null || rollback_failed=1
  fi
  if [ "${OLD_ROOT_MOVED}" = 1 ] && [ -d "${LEGACY}" ]; then
    mv -- "${LEGACY}" "${ROOT}" 2>/dev/null || rollback_failed=1
  fi
  restore_unit_files || rollback_failed=1
  systemctl reload ssh.service >/dev/null 2>&1 || systemctl reload sshd.service >/dev/null 2>&1 || rollback_failed=1
  if [ "${TIMER_WAS_ACTIVE}" = 1 ]; then systemctl start vapor-state-export.timer >/dev/null 2>&1 || rollback_failed=1; fi
  if [ "${rollback_failed}" = 1 ]; then
    report_line rollback_result incomplete
    return 1
  fi
  report_line rollback_result complete
  return 0
}

finish() {
  local status="$?"
  trap - EXIT
  if [ "$status" -ne 0 ] && [ "${APPLYING}" = 1 ] && [ "${MIGRATION_SUCCEEDED}" = 0 ]; then
    if rollback_cutover; then REPORT_RESULT=rolled_back; else REPORT_RESULT=rollback_incomplete; fi
  elif [ "$status" -ne 0 ] && [ "${TIMER_WAS_ACTIVE}" = 1 ]; then
    if systemctl start vapor-state-export.timer >/dev/null 2>&1; then
      REPORT_RESULT=preflight_failed_timer_restored
    else
      REPORT_RESULT=preflight_failed_timer_restore_failed
    fi
  fi
  if [ "${MIGRATION_SUCCEEDED}" = 1 ]; then REPORT_RESULT=success; fi
  if [ -n "${REPORT_DETAIL}" ] && [ "${REPORT_DETAIL}" != none ]; then report_line detail "${REPORT_DETAIL}"; fi
  report_line result "${REPORT_RESULT}"
  report_line finished_at_utc "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  if curl --fail --silent --show-error -H 'Linx-Expiry: 600' -T "${REPORT}" 'https://paste.rtech.support/upload/' >/dev/null; then
    echo "Cutover report: https://paste.rtech.support/selif/vapor-cutover-${REPORT_ID}.txt"
    rm -f -- "${REPORT}"
  else
    echo "Cutover report upload failed; local report remains at ${REPORT}" >&2
  fi
  if [ -d "${CANDIDATE}" ]; then rm -rf -- "${CANDIDATE}"; fi
  exit "$status"
}
trap finish EXIT

exec 9>/run/vapor-cutover.lock
if ! flock -n 9; then fail 'another cutover is already running'; fi
report_line started_at_utc "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
report_line host_os "$(. /etc/os-release && printf '%s' "${PRETTY_NAME:-unknown}")"

if [ ! -d "${ROOT}/.git" ] || [ -L "${ROOT}" ]; then fail 'expected legacy checkout is not present as a regular directory'; fi
old_origin="$(git -C "${ROOT}" remote get-url origin 2>/dev/null || true)"
old_repository="$(safe_origin_repository "${old_origin}")"
if [ "${old_repository}" != GHF-Studios/Vapor-Server-Root ]; then fail 'legacy repository identity changed'; fi
old_revision="$(git -C "${ROOT}" rev-parse HEAD 2>/dev/null || true)"
if [ "${old_revision}" != cd219b979906e776cc4d34dfdb17c8a93481f9b0 ]; then fail 'legacy checkout revision changed since preflight'; fi
old_status="$(git -C "${ROOT}" status --porcelain --untracked-files=all 2>/dev/null || true)"
if [ "${old_status}" != ' M Atlas' ]; then fail 'legacy checkout has additional or changed local files'; fi
atlas_revision="$(git -C "${ROOT}/Atlas" rev-parse HEAD 2>/dev/null || true)"
if [ "${atlas_revision}" != d2a9e72fdd592059beaff8247d70e26cdfeed3e9 ]; then fail 'legacy Atlas checkout differs from the reviewed pin'; fi
report_line legacy_revision "${old_revision}"
report_line legacy_atlas_revision "${atlas_revision}"

temporary_timers="$(systemctl list-units --all --type=timer --no-legend 'vapor-ssh-expire-*.timer' 2>/dev/null || true)"
if [ -n "${temporary_timers}" ]; then fail 'temporary SSH expiry timers exist; resolve grants before migration'; fi

for unit in "${UNITS[@]}" "${TEMPLATE_UNITS[@]}"; do capture_unit_state "${unit}"; done
if [ "${UNIT_ACTIVE[vapor-state-export.timer]}" != active ] || [[ "${UNIT_ENABLED[vapor-state-export.timer]}" != enabled* ]]; then
  fail 'state-export timer is not in the reviewed active and enabled state'
fi
if [[ "${UNIT_ENABLED[vapor-deploy.timer]}" == enabled* ]]; then fail 'legacy periodic deploy timer is enabled'; fi

target_sha="$(git ls-remote "${REPO_URL}" refs/heads/main | awk 'NR == 1 { print $1 }')"
if [[ ! "${target_sha}" =~ ^[0-9a-f]{40}$ ]]; then fail 'could not resolve the current Platform main revision'; fi
report_line target_revision "${target_sha}"

if ! check_ci_success "${target_sha}"; then
  fail 'the current Platform main revision does not have a completed successful CI run'
fi
report_line ci_root_check success

if [ -e "${CANDIDATE}" ] || [ -e "${LEGACY}" ] || [ -e "${FAILED}" ]; then fail 'a cutover staging or backup path already exists'; fi
git clone --no-checkout "${REPO_URL}" "${CANDIDATE}"
git -C "${CANDIDATE}" fetch --no-tags origin main
git -C "${CANDIDATE}" checkout --detach "${target_sha}"
git -C "${CANDIDATE}" submodule sync --recursive
git -C "${CANDIDATE}" submodule update --init --recursive
python3 "${CANDIDATE}/deploy/scripts/verify-deploy-source.py" --root "${CANDIDATE}" --commit "${target_sha}" >/dev/null
if [ -n "$(git -C "${CANDIDATE}" status --porcelain --untracked-files=all)" ]; then fail 'staged Platform checkout is not clean'; fi
candidate_atlas="$(git -C "${CANDIDATE}/Atlas" rev-parse HEAD 2>/dev/null || true)"
if [ "${candidate_atlas}" != "${atlas_revision}" ]; then fail 'staged Platform Atlas pin differs from the preserved legacy checkout'; fi
if [ "$(git ls-remote "${REPO_URL}" refs/heads/main | awk 'NR == 1 { print $1 }')" != "${target_sha}" ]; then fail 'Platform main advanced after staging'; fi

mkdir -p -- "${BACKUP}/systemd" "${BACKUP}/sudoers" "${BACKUP}/local"
chmod 0700 -- "${BACKUP}"
cp -a -- "${CONFIG_ROOT}" "${BACKUP}/vapor-server-config"
cp -a -- /etc/caddy "${BACKUP}/caddy-config"
cp -a -- /etc/ssh/sshd_config "${BACKUP}/sshd_config"
for unit in "${UNITS[@]}" "${TEMPLATE_UNITS[@]}"; do
  if [ "${UNIT_FILE_PRESENT[${unit}]}" = yes ]; then cp -a -- "/etc/systemd/system/${unit}" "${BACKUP}/systemd/${unit}"; fi
done
for path in /etc/sudoers.d/vapor-platform-ops /etc/sudoers.d/vapor-temporary-access; do
  if [ -f "${path}" ] && [ ! -L "${path}" ]; then cp -a -- "${path}" "${BACKUP}/sudoers/"; fi
done
for path in /usr/local/libexec/vapor-platform-ops /usr/local/sbin/vapor-ssh-deploy /usr/local/sbin/vapor-ssh-service-control; do
  if [ -f "${path}" ] && [ ! -L "${path}" ]; then cp -a -- "${path}" "${BACKUP}/local/"; fi
done
if getent group vapor-temp-ssh >/dev/null; then
  TEMP_SSH_GROUP_EXISTED=1
  report_line temp_ssh_group existed
else
  report_line temp_ssh_group absent
fi
if [ -e /srv/vapor-web/atlas ]; then
  if [ -L /srv/vapor-web/atlas ] || [ ! -d /srv/vapor-web/atlas ]; then fail 'Atlas web root is not a regular directory'; fi
  cp -a -- /srv/vapor-web/atlas "${BACKUP}/atlas-web"
fi

TIMER_WAS_ACTIVE=1
systemctl stop vapor-state-export.timer
if systemctl is-active --quiet vapor-state-export.timer; then fail 'state-export timer remained active after stop'; fi
if ! bash "${ROOT}/deploy/scripts/export-state.sh" "${STATE_BUNDLE}"; then fail 'could not create a fresh state export'; fi
STATE_BACKED_UP=1
python3 "${CANDIDATE}/deploy/scripts/state-bundle-validate.py" "${STATE_BUNDLE}"
report_line state_backup "${STATE_BUNDLE}"

APPLYING=1
for unit in caddy.service vapor-homepage.service vapor-docs.service vapor-identity.service vapor-diagnostics.service vapor-registry.service vapor-artifact.service vapor-build-cache.service vapor-platform-server.service vapor-platform-ops.socket; do
  systemctl stop "${unit}" >/dev/null 2>&1 || true
done
mv -- "${ROOT}" "${LEGACY}"
OLD_ROOT_MOVED=1
mv -- "${CANDIDATE}" "${ROOT}"
NEW_ROOT_INSTALLED=1

mode_file="${CONFIG_ROOT}/platform.env"
if [ ! -f "${mode_file}" ] || [ -L "${mode_file}" ]; then fail 'Platform environment file is not a regular file'; fi
mode_tmp="$(mktemp "${mode_file}.XXXXXX")"
awk '!/^VAPOR_SERVER_MODE=/' "${mode_file}" >"${mode_tmp}"
printf 'VAPOR_SERVER_MODE=services\n' >>"${mode_tmp}"
chown --reference="${mode_file}" "${mode_tmp}"
chmod --reference="${mode_file}" "${mode_tmp}"
mv -f -- "${mode_tmp}" "${mode_file}"

VAPOR_DEPLOY_BOOTSTRAP=1 VAPOR_SERVER_MODE=services VAPOR_BRANCH=main VAPOR_REPO_URL="${REPO_URL}" CARGO_BUILD_JOBS=1 \
  bash "${ROOT}/deploy/scripts/deploy.sh" --approved "${target_sha}"

systemctl is-active --quiet vapor-platform-server.service
systemctl is-enabled --quiet vapor-state-export.timer
systemctl is-active --quiet vapor-state-export.timer
local_health="$(curl --silent --show-error --connect-timeout 3 --max-time 10 -o /dev/null -w '%{http_code}' http://127.0.0.1:7110/healthz)"
if [ "${local_health}" != 200 ]; then fail 'local Platform health route failed after deployment'; fi
admin_status="$(curl --silent --show-error --connect-timeout 3 --max-time 10 -o /dev/null -w '%{http_code}' https://vapor.ghf-studios.site/admin/operations)"
if [[ ! "${admin_status}" =~ ^(2[0-9][0-9]|3[0-9][0-9]|401|403)$ ]]; then fail 'public Operations route is unavailable after deployment'; fi
cache_status="$(curl --silent --show-error --connect-timeout 3 --max-time 10 -o /dev/null -w '%{http_code}' https://vapor.ghf-studios.site/api/build-cache/v1/status)"
if [ "${cache_status}" != 200 ]; then fail 'public Build Cache status route is unavailable after deployment'; fi
REPORT_DETAIL="deployment healthy; Platform=${target_sha}; Operations=${admin_status}; BuildCache=${cache_status}"
report_line deployment_revision "${target_sha}"
report_line local_health "${local_health}"
report_line public_operations "${admin_status}"
report_line public_build_cache "${cache_status}"
if [ "${TIMER_WAS_ACTIVE}" = 1 ]; then systemctl start vapor-state-export.timer; TIMER_WAS_ACTIVE=0; fi
MIGRATION_SUCCEEDED=1
APPLYING=0
echo "cutover: Platform ${target_sha} is healthy; legacy checkout preserved at ${LEGACY}"
