#!/usr/bin/env bash
set -euo pipefail
umask 077

report_id="${1:-}"
if [ "$#" -ne 1 ] || [[ ! "$report_id" =~ ^[a-f0-9]{16}$ ]]; then
  echo 'Provide a one-time 16-character hexadecimal report code.' >&2
  exit 1
fi
filename="vpf-${report_id}.txt"
report="/tmp/${filename}"
if [ -e "$report" ]; then
  echo 'A preflight report already exists for this second; retry in one second.' >&2
  exit 1
fi
trap 'rm -f -- "$report"' EXIT

if [ "$(id -u)" -ne 0 ]; then
  echo 'Run with sudo; this report reads no secret values and changes no host state.' >&2
  exit 1
fi

{
  printf 'captured_at_utc=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  if [ -r /etc/os-release ]; then
    . /etc/os-release
    printf 'os=%s\n' "${PRETTY_NAME:-unknown}"
  else
    printf 'os=unknown\n'
  fi
  printf 'kernel=%s\n' "$(uname -r)"
  printf 'online_cpu_count=%s\n' "$(getconf _NPROCESSORS_ONLN 2>/dev/null || printf 'unknown')"
  printf 'memory_total_kib=%s\n' "$(awk '/^MemTotal:/ { print $2; found=1 } END { if (!found) print "unknown" }' /proc/meminfo 2>/dev/null)"
  printf 'root_free_bytes=%s\n' "$(df -B1 --output=avail / 2>/dev/null | tail -n 1 | tr -d ' ' || printf 'unknown')"
  printf 'state_free_bytes=%s\n' "$(df -B1 --output=avail /var/lib/vapor-server 2>/dev/null | tail -n 1 | tr -d ' ' || printf 'unknown')"
  printf 'host_virtualization=%s\n' "$(systemd-detect-virt 2>/dev/null || printf 'none-or-unavailable')"
  printf 'kvm_device=%s\n' "$([ -c /dev/kvm ] && printf 'available' || printf 'unavailable')"
  printf 'cpu_hardware_virtualization=%s\n' "$(grep -m1 -oE '\b(vmx|svm)\b' /proc/cpuinfo 2>/dev/null | head -n 1 || printf 'unavailable')"
  printf 'qemu_x86_64=%s\n' "$(command -v qemu-system-x86_64 2>/dev/null || printf 'not-installed')"
  printf 'windows_guest_license=operator_confirmation_required\n'

  for path in \
    /opt/vapor-platform-server \
    /opt/vapor-server-root \
    /etc/vapor-server \
    /etc/vapor-server/platform.env \
    /etc/caddy/Caddyfile \
    /var/lib/vapor-server \
    /var/backups/vapor-server; do
    if [ -e "$path" ] || [ -L "$path" ]; then
      if [ -L "$path" ]; then kind=symlink; elif [ -d "$path" ]; then kind=directory; else kind=file; fi
      printf 'path=%s exists=yes type=%s metadata=' "$path" "$kind"
      stat -c '%U:%G:%a:%y' -- "$path" 2>/dev/null || printf 'unavailable'
      printf '\n'
    else
      printf 'path=%s exists=no\n' "$path"
    fi
  done

  for root in /opt/vapor-platform-server /opt/vapor-server-root; do
    [ -d "$root/.git" ] || continue
    printf 'checkout=%s\n' "$root"
    origin="$(git -C "$root" remote get-url origin 2>/dev/null || true)"
    case "$origin" in
      https://github.com/GHF-Studios/Vapor-Platform-Server.git|git@github.com:GHF-Studios/Vapor-Platform-Server.git) origin_state=expected ;;
      '') origin_state=unavailable ;;
      *) origin_state=unexpected ;;
    esac
    printf 'origin_state=%s\n' "$origin_state"
    printf 'branch=%s\n' "$(git -C "$root" branch --show-current 2>/dev/null || printf 'detached-or-unavailable')"
    printf 'revision=%s\n' "$(git -C "$root" rev-parse HEAD 2>/dev/null || printf 'unavailable')"
    if status="$(git -C "$root" status --porcelain --untracked-files=all 2>/dev/null)"; then
      count="$(printf '%s\n' "$status" | awk 'NF { n++ } END { print n+0 }')"
      printf 'working_tree_entries=%s\n' "$count"
    else
      printf 'working_tree_entries=unavailable\n'
    fi
    if [ -f "$root/.gitmodules" ]; then
      echo 'submodules_begin'
      git -C "$root" submodule status --recursive 2>/dev/null || echo 'submodule_status=unavailable'
      echo 'submodules_end'
    fi
  done

  echo 'temporary_ssh_timers_begin'
  systemctl list-units --all --type=timer --no-legend 'vapor-ssh-expire-*.timer' 2>/dev/null |
    awk 'NR <= 100 { print $1, $3, $4 } END { if (NR > 100) print "truncated_count=" NR }'
  echo 'temporary_ssh_timers_end'

  for unit in \
    caddy.service vapor-platform-server.service vapor-homepage.service \
    vapor-docs.service vapor-identity.service vapor-diagnostics.service \
    vapor-registry.service vapor-artifact.service vapor-build-cache.service \
    vapor-deploy.service vapor-deploy.timer vapor-state-export.service \
    vapor-state-export.timer; do
    load="$(systemctl show --property=LoadState --value "$unit" 2>/dev/null || printf 'unavailable')"
    active="$(systemctl show --property=ActiveState --value "$unit" 2>/dev/null || printf 'unavailable')"
    enabled="$(systemctl is-enabled "$unit" 2>/dev/null || printf 'not-enabled-or-absent')"
    printf 'unit=%s load=%s active=%s enabled=%s\n' "$unit" "$load" "$active" "$enabled"
  done

  for url in \
    http://127.0.0.1:7110/healthz \
    http://127.0.0.1:7110/admin/operations \
    http://127.0.0.1:7110/api/admin/operations \
    http://127.0.0.1:7110/api/build-cache/v1/status \
    http://127.0.0.1:7110/docs \
    https://vapor.ghf-studios.site/healthz \
    https://vapor.ghf-studios.site/admin/operations \
    https://vapor.ghf-studios.site/api/admin/operations \
    https://vapor.ghf-studios.site/api/build-cache/v1/status \
    https://vapor.ghf-studios.site/docs; do
    code="$(curl --silent --show-error --output /dev/null --write-out '%{http_code}' --connect-timeout 2 --max-time 5 "$url" 2>/dev/null || printf '000')"
    printf 'http_status=%s code=%s\n' "$url" "$code"
  done
} >"$report"

if curl --fail --silent --show-error -H 'Linx-Expiry: 600' -T "$report" 'https://paste.rtech.support/upload/' >/dev/null; then
  printf 'Preflight report: https://paste.rtech.support/selif/%s\n' "$filename"
else
  echo 'Report upload failed; no host changes were made.' >&2
  exit 1
fi
