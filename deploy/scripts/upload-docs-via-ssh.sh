#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'USAGE'
usage: upload-docs-via-ssh.sh --bundle PATH --host HOST [--user USER] [--ssh-key PATH] [--ssh-option OPTION]

Stages the public docs bundle with SCP, then posts it from the VPS to its local
Vapor docs service. The admin token is read only on the VPS from
/etc/vapor-server/docs.env.
USAGE
}

BUNDLE=""
HOST=""
SSH_USER="root"
SSH_KEY=""
SSH_OPTIONS=()

while [ "$#" -gt 0 ]; do
  case "$1" in
    --bundle)
      if [ "$#" -lt 2 ]; then usage; exit 2; fi
      BUNDLE="$2"
      shift 2
      ;;
    --host)
      if [ "$#" -lt 2 ]; then usage; exit 2; fi
      HOST="$2"
      shift 2
      ;;
    --user)
      if [ "$#" -lt 2 ]; then usage; exit 2; fi
      SSH_USER="$2"
      shift 2
      ;;
    --ssh-key)
      if [ "$#" -lt 2 ]; then usage; exit 2; fi
      SSH_KEY="$2"
      shift 2
      ;;
    --ssh-option)
      if [ "$#" -lt 2 ]; then usage; exit 2; fi
      SSH_OPTIONS+=("$2")
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

if [ -z "${BUNDLE}" ] || [ -z "${HOST}" ]; then
  usage
  exit 2
fi
if [ ! -f "${BUNDLE}" ]; then
  echo "error: bundle does not exist: ${BUNDLE}" >&2
  exit 1
fi

SSH_ARGS=()
if [ -n "${SSH_KEY}" ]; then
  SSH_ARGS+=("-i" "${SSH_KEY}")
fi
for option in "${SSH_OPTIONS[@]}"; do
  SSH_ARGS+=("-o" "${option}")
done

TARGET="${SSH_USER}@${HOST}"
REMOTE_DIR="$(ssh "${SSH_ARGS[@]}" "${TARGET}" 'umask 077 && mktemp -d /tmp/vapor-docs-upload.XXXXXXXX')"
if [[ ! "${REMOTE_DIR}" =~ ^/tmp/vapor-docs-upload\.[A-Za-z0-9]+$ ]]; then
  echo "error: VPS returned an unexpected temporary directory" >&2
  exit 1
fi
REMOTE_BUNDLE="${REMOTE_DIR}/docs.tar.gz"

cleanup_remote() {
  local status=$?
  trap - EXIT
  ssh "${SSH_ARGS[@]}" "${TARGET}" "rm -f -- '${REMOTE_BUNDLE}' && rmdir -- '${REMOTE_DIR}'" \
    >/dev/null 2>&1 || true
  exit "${status}"
}
trap cleanup_remote EXIT

scp "${SSH_ARGS[@]}" -- "${BUNDLE}" "${TARGET}:${REMOTE_BUNDLE}"

# Keep the script out of stdin: sudo needs the SSH pty to prompt for the
# operator's password. The base64 payload contains no secret and is decoded
# only on the VPS, after sudo has validated the operator.
REMOTE_SCRIPT="$(cat <<'REMOTE'
set -euo pipefail
bundle="$1"
config=/etc/vapor-server/docs.env
if [[ ! -r "${config}" ]]; then
  echo "error: missing ${config}" >&2
  exit 1
fi
set -a
# shellcheck disable=SC1090
. "${config}"
set +a
if [[ -z "${VAPOR_DOCS_ADMIN_TOKEN:-}" ]]; then
  echo 'error: VAPOR_DOCS_ADMIN_TOKEN is empty' >&2
  exit 1
fi
curl --fail --silent --show-error \
  --request POST \
  --header "Authorization: Bearer ${VAPOR_DOCS_ADMIN_TOKEN}" \
  --data-binary "@${bundle}" \
  http://127.0.0.1:7112/current.tar.gz
REMOTE
)"
REMOTE_SCRIPT_B64="$(printf '%s' "${REMOTE_SCRIPT}" | base64 | tr -d '\n')"
PRIVILEGED_COMMAND="sudo -v && printf '%s' '${REMOTE_SCRIPT_B64}' | base64 -d | sudo -n bash -s -- '${REMOTE_BUNDLE}'"
ssh -t "${SSH_ARGS[@]}" "${TARGET}" "${PRIVILEGED_COMMAND}"
