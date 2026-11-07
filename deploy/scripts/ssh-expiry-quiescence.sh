#!/usr/bin/env bash

# Callers must declare ACTIVE_SSH_EXPIRY_TIMERS before sourcing this file.
pause_temporary_ssh_expiry() {
  local timer
  local deadline
  local active_services
  local queued_jobs
  local timer_output
  local -a timers=()
  timer_output="$(systemctl list-units --type=timer --state=active --no-legend \
    'vapor-ssh-expire-*.timer')" || {
    echo "error: could not inspect temporary SSH expiry timers; recovery operation aborted" >&2
    return 1
  }
  mapfile -t timers < <(printf '%s\n' "${timer_output}" | awk 'NF { print $1 }')
  for timer in "${timers[@]}"; do
    ACTIVE_SSH_EXPIRY_TIMERS+=("${timer}")
    if ! systemctl stop "${timer}"; then
      echo "error: could not pause temporary SSH expiry timer ${timer}; recovery operation aborted" >&2
      return 1
    fi
  done

  # Stopping a timer does not cancel an expiry service it already queued.
  # Drain those root helpers before changing the Platform SQLite snapshot.
  deadline=$((SECONDS + 60))
  while true; do
    if ! active_services="$(systemctl list-units --type=service \
      --state=active,activating,deactivating --no-legend \
      'vapor-ssh-expire-*.service' | awk 'NF { print $1 }')"; then
      echo "error: could not inspect temporary SSH expiry services; recovery operation aborted" >&2
      return 1
    fi
    if ! queued_jobs="$(systemctl list-jobs --no-legend \
      | awk '$2 ~ /^vapor-ssh-expire-/ { print $2 }')"; then
      echo "error: could not inspect queued temporary SSH expiry work; recovery operation aborted" >&2
      return 1
    fi
    if [ -z "${active_services}" ] && [ -z "${queued_jobs}" ]; then
      return 0
    fi
    if [ "${SECONDS}" -ge "${deadline}" ]; then
      echo "error: temporary SSH expiry work did not quiesce; recovery operation aborted" >&2
      return 1
    fi
    sleep 1
  done
}

restart_temporary_ssh_expiry() {
  local timer
  local failed=0
  for timer in "${ACTIVE_SSH_EXPIRY_TIMERS[@]}"; do
    if ! systemctl start "${timer}"; then
      echo "error: could not restart a temporary SSH expiry timer after recovery operation" >&2
      failed=1
    fi
  done
  return "${failed}"
}
