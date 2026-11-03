#!/usr/bin/env bash
set -Eeuo pipefail
umask 077

# One-time, operator-run bootstrap for the requester-initiated SSH flow.
# This script is intentionally pinned to the reviewed Platform/Identity commits.
readonly ROOT=/opt/vapor-platform-server
readonly REPOSITORY=https://github.com/GHF-Studios/Vapor-Platform-Server.git
readonly TARGET_COMMIT=d7c1645a1ee6928b73f888ecc5deeb183d730a39
readonly TARGET_IDENTITY=0efc105be16df13c8d388496de5a32714a97b694
readonly STATE_ROOT=/var/lib/vapor-server
readonly CONFIG_ROOT=/etc/vapor-server
readonly BACKUP_ROOT=/var/backups/vapor-server
readonly ATLAS_ROOT=/srv/vapor-web/atlas
readonly SERVICE_UNITS=(
  vapor-platform-server.service vapor-homepage.service vapor-docs.service
  vapor-identity.service vapor-diagnostics.service vapor-registry.service
  vapor-artifact.service vapor-build-cache.service caddy.service
)

stamp="$(date -u +%Y%m%dT%H%M%SZ)"
backup="${BACKUP_ROOT}/ssh-bootstrap-${stamp}"
candidate="${ROOT}.candidate-${stamp}"
legacy="${ROOT}.previous-${stamp}"
failed="${ROOT}.failed-${stamp}"
report=""
result=preflight_failed
detail=not_started
old_commit=unknown
state_backup=not_created
state_timer_was_active=0
old_root_moved=0
new_root_installed=0
temporary_ssh_group_existed=0
atlas_existed=0
applying=0

line() { printf '%s=%s\n' "$1" "$2" >>"${report}"; }
fail() { detail="$1"; echo "bootstrap: ${detail}" >&2; exit 1; }

restore_directory() {
  local source="$1" destination="$2" failed_copy="${destination}.ssh-bootstrap-failed-${stamp}"
  if [ ! -d "${source}" ] || [ -L "${source}" ]; then return 1; fi
  if [ -e "${destination}" ] || [ -L "${destination}" ]; then
    if [ -e "${failed_copy}" ] || [ -L "${failed_copy}" ]; then return 1; fi
    mv -- "${destination}" "${failed_copy}" || return 1
  fi
  cp -a -- "${source}" "${destination}"
}

rollback() {
  local rollback_failed=0 unit
  echo 'bootstrap: deployment failed; attempting rollback' >&2
  for unit in "${SERVICE_UNITS[@]}"; do systemctl stop "${unit}" >/dev/null 2>&1 || true; done

  if [ "${new_root_installed}" -eq 1 ]; then
    if [ -d "${ROOT}" ] && [ ! -L "${ROOT}" ] && [ ! -e "${failed}" ]; then
      mv -- "${ROOT}" "${failed}" || rollback_failed=1
    else
      rollback_failed=1
    fi
  fi
  if [ "${old_root_moved}" -eq 1 ]; then
    if [ -d "${legacy}" ] && [ ! -L "${legacy}" ] && [ ! -e "${ROOT}" ]; then
      mv -- "${legacy}" "${ROOT}" || rollback_failed=1
    else
      rollback_failed=1
    fi
  fi

  if [ -d "${ROOT}/.git" ] && [ "${old_commit}" != unknown ]; then
    if ! VAPOR_DEPLOY_BOOTSTRAP=1 VAPOR_SERVER_MODE=monolith VAPOR_BRANCH=main \
      VAPOR_REPO_URL="${REPOSITORY}" CARGO_BUILD_JOBS=1 \
      bash "${ROOT}/deploy/scripts/deploy.sh" --approved "${old_commit}"; then
      rollback_failed=1
    fi
    restore_directory "${backup}/config/vapor-server" /etc/vapor-server || rollback_failed=1
    restore_directory "${backup}/config/caddy" /etc/caddy || rollback_failed=1
    if [ "${atlas_existed}" -eq 1 ]; then
      restore_directory "${backup}/atlas-web" "${ATLAS_ROOT}" || rollback_failed=1
    elif [ -e "${ATLAS_ROOT}" ] || [ -L "${ATLAS_ROOT}" ]; then
      if [ ! -L "${ATLAS_ROOT}" ] && [ ! -e "${backup}/atlas-web-created-by-failed-cutover" ]; then
        mv -- "${ATLAS_ROOT}" "${backup}/atlas-web-created-by-failed-cutover" || rollback_failed=1
      else
        rollback_failed=1
      fi
    fi
    if [ -f "${backup}/config/sshd_config" ]; then
      cp -a -- "${backup}/config/sshd_config" /etc/ssh/sshd_config || rollback_failed=1
    else
      rollback_failed=1
    fi
    for name in vapor-platform-ops vapor-temporary-access; do
      if [ -f "${backup}/sudoers/${name}" ]; then
        cp -a -- "${backup}/sudoers/${name}" "/etc/sudoers.d/${name}" || rollback_failed=1
      else
        rm -f -- "/etc/sudoers.d/${name}" || rollback_failed=1
      fi
    done
    restore_local_file() {
      local path="$1" saved="$2"
      if [ -f "${saved}" ]; then cp -a -- "${saved}" "${path}" || return 1
      else rm -f -- "${path}" || return 1
      fi
    }
    restore_local_file /usr/local/libexec/vapor-platform-ops "${backup}/local/vapor-platform-ops" || rollback_failed=1
    restore_local_file /usr/local/sbin/vapor-ssh-deploy "${backup}/local/vapor-ssh-deploy" || rollback_failed=1
    restore_local_file /usr/local/sbin/vapor-ssh-service-control "${backup}/local/vapor-ssh-service-control" || rollback_failed=1
    if [ -f "${backup}/systemd/vapor-deploy@.service" ]; then
      cp -a -- "${backup}/systemd/vapor-deploy@.service" /etc/systemd/system/vapor-deploy@.service || rollback_failed=1
    else
      rm -f -- /etc/systemd/system/vapor-deploy@.service || rollback_failed=1
    fi
    systemctl daemon-reload || rollback_failed=1
    if [ "${temporary_ssh_group_existed}" -eq 0 ] && getent group vapor-temp-ssh >/dev/null; then
      members="$(getent group vapor-temp-ssh | awk -F: '{print $4}')"
      if [ -z "${members}" ]; then groupdel vapor-temp-ssh || rollback_failed=1
      else rollback_failed=1
      fi
    fi
    if ! visudo -c >/dev/null 2>&1 || ! sshd -t; then rollback_failed=1; fi
    systemctl reload ssh.service >/dev/null 2>&1 || systemctl reload sshd.service >/dev/null 2>&1 || rollback_failed=1
    systemctl restart caddy.service >/dev/null 2>&1 || rollback_failed=1
    if [ "${state_backup}" != not_created ]; then
      if ! bash "${ROOT}/deploy/scripts/restore-state.sh" --bundle "${state_backup}" --yes; then
        rollback_failed=1
      fi
    fi
    if ! bash "${ROOT}/deploy/scripts/health-check.sh"; then rollback_failed=1; fi
  else
    rollback_failed=1
  fi

  if [ "${state_timer_was_active}" -eq 1 ]; then
    systemctl start vapor-state-export.timer >/dev/null 2>&1 || rollback_failed=1
  fi
  if [ "${rollback_failed}" -eq 0 ]; then
    result=rolled_back
    detail=deployment_failed_old_revision_restored
  else
    result=rollback_incomplete
    detail=manual_recovery_required_backup_preserved
  fi
  line rollback_result "${result}"
  return "${rollback_failed}"
}

finish() {
  local exit_status=$?
  trap - EXIT
  set +e
  if [ "${applying}" -eq 1 ] && [ "${result}" != succeeded ]; then rollback; fi
  if [ "${state_timer_was_active}" -eq 1 ] && ! systemctl is-active --quiet vapor-state-export.timer; then
    systemctl start vapor-state-export.timer >/dev/null 2>&1 || { result=rollback_incomplete; detail=state_export_timer_could_not_be_restored; exit_status=1; }
  fi
  if [ "${result}" = preflight_failed ]; then result=preflight_failed; fi
  line result "${result}"
  line detail "${detail}"
  line target_commit "${TARGET_COMMIT}"
  line identity_commit "${TARGET_IDENTITY}"
  line previous_commit "${old_commit}"
  line backup_path "${backup}"
  line candidate_path "${candidate}"
  line atlas_backup "${atlas_existed}"
  line state_backup "${state_backup}"
  line deploy_timer "left_unchanged_and_not_started"
  if command -v curl >/dev/null 2>&1 && [ -n "${report}" ] && [ -f "${report}" ]; then
    paste_url="$(curl --fail --silent --show-error --connect-timeout 5 --max-time 20 \
      --data-binary "@${report}" https://paste.rs/ 2>/dev/null)"
    if [[ "${paste_url}" =~ ^https://paste\.rs/[A-Za-z0-9]+$ ]]; then
      echo "bootstrap: report uploaded (${paste_url}); it will be deleted after review"
    else
      echo "bootstrap: report upload failed; report remains at ${report}" >&2
    fi
  else
    echo "bootstrap: curl unavailable for report upload; report remains at ${report}" >&2
  fi
  echo "bootstrap: ${result} (${detail})"
  exit "${exit_status}"
}
trap finish EXIT

report="$(mktemp /tmp/vapor-ssh-bootstrap.XXXXXX)"
chmod 0600 "${report}"
line started_at_utc "${stamp}"

[ "$(id -u)" -eq 0 ] || fail run_with_sudo
grep -q '^ID=ubuntu$' /etc/os-release || fail expected_ubuntu
for command in git python3 curl flock systemctl visudo sshd df awk groupadd groupdel getent; do
  command -v "${command}" >/dev/null 2>&1 || fail "missing_${command}"
done
exec 9>/run/vapor-ssh-endpoint-bootstrap.lock
flock -n 9 || fail another_ssh_endpoint_bootstrap_is_running
[ -d "${ROOT}/.git" ] && [ ! -L "${ROOT}" ] || fail existing_platform_checkout_missing_or_not_regular
origin="$(git -C "${ROOT}" remote get-url origin 2>/dev/null || true)"
[ "${origin}" = "${REPOSITORY}" ] || fail existing_checkout_origin_unexpected
old_commit="$(git -C "${ROOT}" rev-parse HEAD 2>/dev/null || true)"
[[ "${old_commit}" =~ ^[0-9a-f]{40}$ ]] || fail existing_checkout_revision_unavailable
[ -z "$(git -C "${ROOT}" status --porcelain --untracked-files=all 2>/dev/null)" ] || fail existing_checkout_has_local_changes

if systemctl is-active --quiet vapor-deploy.timer; then fail deploy_timer_is_active_abort_to_avoid_race; fi
if systemctl is-enabled --quiet vapor-deploy.timer; then fail deploy_timer_is_enabled_abort_to_keep_it_retired; fi
if systemctl is-active --quiet vapor-deploy.service; then fail deploy_service_is_active_abort_to_avoid_race; fi
active_workers="$(systemctl list-units --state=active --no-legend 'vapor-deploy@*.service' 2>/dev/null || true)"
if [ -n "${active_workers}" ]; then
  fail approved_deploy_worker_is_active_abort_to_avoid_race
fi
if [ ! -f "${CONFIG_ROOT}/platform.env" ] || [ -L "${CONFIG_ROOT}/platform.env" ]; then
  fail platform_environment_file_missing_or_not_regular
fi

git -C "${ROOT}" fetch --no-tags origin refs/heads/main:refs/remotes/origin/main >/dev/null || fail fetch_main_failed
[[ "$(git -C "${ROOT}" rev-parse origin/main)" = "${TARGET_COMMIT}" ]] || fail main_advanced_or_differs_from_reviewed_commit
git -C "${ROOT}" cat-file -e "${TARGET_COMMIT}^{commit}" 2>/dev/null || fail reviewed_target_commit_not_available
python3 "${ROOT}/deploy/scripts/verify-deploy-source.py" --root "${ROOT}" --commit "${TARGET_COMMIT}" >/dev/null \
  || fail reviewed_source_or_submodule_origins_invalid
identity_pin="$(git -C "${ROOT}" ls-tree "${TARGET_COMMIT}" Vapor-Identity-Server | awk '{print $3}')"
[ "${identity_pin}" = "${TARGET_IDENTITY}" ] || fail identity_submodule_pin_mismatch
ci_runs="$(curl --fail --silent --show-error --connect-timeout 5 --max-time 20 \
  "https://api.github.com/repos/GHF-Studios/Vapor-Platform-Server/actions/runs?head_sha=${TARGET_COMMIT}&event=push&per_page=50" \
  -H 'Accept: application/vnd.github+json' -H 'User-Agent: vapor-ssh-endpoint-bootstrap')" || fail github_ci_status_unavailable
printf '%s' "${ci_runs}" | python3 -c 'import json,sys; runs=json.load(sys.stdin).get("workflow_runs", []); ok=any(r.get("path", "").endswith("ci.yml") and r.get("status")=="completed" and r.get("conclusion")=="success" for r in runs); sys.exit(0 if ok else 1)' \
  || fail reviewed_commit_has_no_successful_platform_ci
queue_db="${STATE_ROOT}/platform/platform.sqlite3"
if [ -f "${queue_db}" ] && [ ! -L "${queue_db}" ]; then
  python3 - "${queue_db}" <<'PY' || fail an_approved_or_running_deploy_request_exists
import sqlite3, sys
db = sqlite3.connect(f"file:{sys.argv[1]}?mode=ro", uri=True, timeout=2)
exists = db.execute("SELECT 1 FROM sqlite_master WHERE type='table' AND name='platform_deploy_requests'").fetchone()
if exists:
    active = db.execute("SELECT COUNT(*) FROM platform_deploy_requests WHERE status IN ('approved','executing')").fetchone()[0]
    sys.exit(1 if active else 0)
PY
fi

free_bytes="$(df -PB1 /opt | awk 'NR == 2 { print $4 }')"
[[ "${free_bytes}" =~ ^[0-9]+$ ]] && [ "${free_bytes}" -ge 8589934592 ] || fail less_than_8_gibibytes_free_on_opt
if [ -e "${candidate}" ] || [ -L "${candidate}" ] || [ -e "${legacy}" ] || [ -L "${legacy}" ] || [ -e "${failed}" ] || [ -L "${failed}" ]; then
  fail staging_or_backup_path_already_exists
fi
if [ -e "${backup}" ] || [ -L "${backup}" ]; then fail backup_path_already_exists; fi
atlas_config="$(awk -F= '$1 == "VAPOR_ATLAS_WEB_ROOT" {value=$2; gsub(/^[[:space:]\"]+|[[:space:]\"]+$/, "", value); print value; exit}' "${CONFIG_ROOT}/platform.env")"
if [ -n "${atlas_config}" ] && [ "${atlas_config}" != "${ATLAS_ROOT}" ]; then fail atlas_root_differs_from_guarded_path; fi

install -d -o root -g root -m 0700 "${backup}" "${backup}/config" "${backup}/sudoers"
install -d -o root -g root -m 0700 "${backup}/local" "${backup}/systemd"
cp -a -- "${CONFIG_ROOT}" "${backup}/config/vapor-server" || fail config_backup_failed
[ -d /etc/caddy ] && [ ! -L /etc/caddy ] && cp -a -- /etc/caddy "${backup}/config/caddy" || fail caddy_config_backup_failed
[ ! -e "${ATLAS_ROOT}" ] && [ ! -L "${ATLAS_ROOT}" ] || {
  [ -d "${ATLAS_ROOT}" ] && [ ! -L "${ATLAS_ROOT}" ] || fail atlas_root_is_not_a_regular_directory
  cp -a -- "${ATLAS_ROOT}" "${backup}/atlas-web" || fail atlas_backup_failed
  atlas_existed=1
}
[ -f /etc/ssh/sshd_config ] && [ ! -L /etc/ssh/sshd_config ] && cp -a -- /etc/ssh/sshd_config "${backup}/config/sshd_config" || fail ssh_config_backup_failed
for name in vapor-platform-ops vapor-temporary-access; do
  if [ -f "/etc/sudoers.d/${name}" ] && [ ! -L "/etc/sudoers.d/${name}" ]; then
    cp -a -- "/etc/sudoers.d/${name}" "${backup}/sudoers/${name}" || fail sudoers_backup_failed
  fi
done
for path in /usr/local/libexec/vapor-platform-ops /usr/local/sbin/vapor-ssh-deploy /usr/local/sbin/vapor-ssh-service-control; do
  if [ -e "${path}" ] || [ -L "${path}" ]; then
    [ -f "${path}" ] && [ ! -L "${path}" ] || fail "unexpected_local_helper_type_$(basename "${path}")"
    cp -a -- "${path}" "${backup}/local/$(basename "${path}")" || fail local_helper_backup_failed
  fi
done
if getent group vapor-temp-ssh >/dev/null; then temporary_ssh_group_existed=1; fi
if [ -e /etc/systemd/system/vapor-deploy@.service ] || [ -L /etc/systemd/system/vapor-deploy@.service ]; then
  [ -f /etc/systemd/system/vapor-deploy@.service ] && [ ! -L /etc/systemd/system/vapor-deploy@.service ] \
    || fail unexpected_approved_deploy_unit_type
  cp -a -- /etc/systemd/system/vapor-deploy@.service "${backup}/systemd/vapor-deploy@.service" || fail approved_deploy_unit_backup_failed
fi

if ! systemctl is-active --quiet vapor-state-export.timer || ! systemctl is-enabled --quiet vapor-state-export.timer; then
  fail state_export_timer_not_active_and_enabled
fi

if systemctl is-active --quiet vapor-state-export.timer; then
  state_timer_was_active=1
  systemctl stop vapor-state-export.timer || fail could_not_pause_state_export_timer
fi
state_backup="${backup}/state.tar.gz"
if ! bash "${ROOT}/deploy/scripts/export-state.sh" "${state_backup}"; then
  state_backup=not_created
  fail state_export_failed
fi
python3 "${ROOT}/deploy/scripts/state-bundle-validate.py" "${state_backup}" >/dev/null || fail state_bundle_validation_failed
systemctl start vapor-state-export.timer || fail could_not_resume_state_export_timer_after_backup

git clone --no-checkout "${REPOSITORY}" "${candidate}" >/dev/null || fail candidate_clone_failed
git -C "${candidate}" fetch --no-tags origin refs/heads/main:refs/remotes/origin/main >/dev/null || fail candidate_fetch_failed
[[ "$(git -C "${candidate}" rev-parse origin/main)" = "${TARGET_COMMIT}" ]] || fail main_advanced_while_candidate_was_staged
git -C "${candidate}" checkout --detach "${TARGET_COMMIT}" >/dev/null || fail candidate_checkout_failed
git -C "${candidate}" submodule sync --recursive >/dev/null || fail submodule_sync_failed
git -C "${candidate}" submodule update --init --recursive || fail submodule_update_failed
python3 "${candidate}/deploy/scripts/verify-deploy-source.py" --root "${candidate}" --commit "${TARGET_COMMIT}" >/dev/null \
  || fail candidate_source_verification_failed
candidate_identity="$(git -C "${candidate}/Vapor-Identity-Server" rev-parse HEAD)"
[ "${candidate_identity}" = "${TARGET_IDENTITY}" ] || fail candidate_identity_revision_mismatch
[ -z "$(git -C "${candidate}" status --porcelain --untracked-files=all)" ] || fail candidate_checkout_not_clean

line preflight passed
line target_on_main yes
line source_ci verified_previously_for_exact_commit
line platform_mode monolith
line state_timer_was_active "${state_timer_was_active}"
applying=1
mv -- "${ROOT}" "${legacy}" || fail could_not_preserve_existing_checkout
old_root_moved=1
mv -- "${candidate}" "${ROOT}" || fail could_not_install_candidate_checkout
new_root_installed=1

if ! VAPOR_DEPLOY_BOOTSTRAP=1 VAPOR_SERVER_MODE=monolith VAPOR_BRANCH=main \
  VAPOR_REPO_URL="${REPOSITORY}" CARGO_BUILD_JOBS=1 \
  bash "${ROOT}/deploy/scripts/deploy.sh" --approved "${TARGET_COMMIT}"; then
  detail=deployment_command_failed
  exit 1
fi

systemctl is-active --quiet vapor-platform-server.service || fail platform_service_not_active
systemctl is-active --quiet caddy.service || fail caddy_service_not_active
local_health="$(curl --silent --show-error --connect-timeout 3 --max-time 10 -o /dev/null -w '%{http_code}' http://127.0.0.1:7110/healthz)"
[ "${local_health}" = 200 ] || fail platform_local_health_failed
ssh_route="$(curl --silent --show-error --connect-timeout 3 --max-time 10 -o /dev/null -w '%{http_code}' \
  -H 'content-type: application/json' --data '{}' \
  http://127.0.0.1:7110/api/identity/v1/cli/ssh-authorization/start)"
[[ "${ssh_route}" =~ ^(400|422)$ ]] || fail ssh_authorization_route_not_ready
ops_route="$(curl --silent --show-error --connect-timeout 3 --max-time 10 -o /dev/null -w '%{http_code}' \
  --resolve vapor.ghf-studios.site:443:127.0.0.1 https://vapor.ghf-studios.site/admin/operations)"
[[ "${ops_route}" =~ ^(200|301|302|401|403)$ ]] || fail public_operations_route_not_ready

if [ "${state_timer_was_active}" -eq 1 ]; then systemctl start vapor-state-export.timer || fail state_export_timer_restore_failed; fi
systemctl is-active --quiet vapor-deploy.timer && fail deploy_timer_unexpectedly_active
line local_health "${local_health}"
line ssh_authorization_route_status "${ssh_route}"
line public_operations_route_status "${ops_route}"
result=succeeded
detail=platform_and_identity_ssh_routes_ready
applying=0
echo 'bootstrap: deployed the reviewed SSH endpoint commit; no SSH grant was created.'
