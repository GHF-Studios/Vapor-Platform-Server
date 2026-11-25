#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
Usage:
  sudo bash install-vapor-release-publisher.sh PUBLISHER_SSH_PUBLIC_KEY RELEASE_SIGNING_PUBLIC_KEY

Creates the dedicated restricted SSH account used to publish signed Vapor
release bundles. It installs one forced-command Ed25519 key and a sudo rule for
the root-owned bundle receiver only.
EOF
}

if [[ "$#" -ne 2 ]]; then
  usage
  exit 2
fi
if [[ "$(id -u)" -ne 0 ]]; then
  echo "error: run this installer as root" >&2
  exit 1
fi

ssh_key_input="$1"
release_key_input="$2"
for path in "$ssh_key_input" "$release_key_input"; do
  if [[ ! -f "$path" || -L "$path" ]]; then
    echo "error: expected a regular, non-symlink file: $path" >&2
    exit 1
  fi
done
for command_name in ssh-keygen openssl visudo useradd userdel install sudo getent tail od tr; do
  command -v "$command_name" >/dev/null 2>&1 || {
    echo "error: required system command is unavailable: $command_name" >&2
    exit 1
  }
done

username=vapor-release-publisher
home=/var/lib/vapor-release-publisher
receiver=/usr/local/sbin/vapor-release-publish-receiver
sudoers=/etc/sudoers.d/vapor-release-publisher
release_key_dir=/etc/vapor-server/release-keys
release_key="$release_key_dir/vapor-release-2026-11-24.pem"
checkout="${VAPOR_PLATFORM_CHECKOUT:-/opt/vapor-platform-server}"
receiver_source="$checkout/deploy/scripts/vapor-release-publish-receiver.sh"
if [[ ! -f "$receiver_source" || -L "$receiver_source" ]]; then
  echo "error: deployed receiver source is unavailable: $receiver_source" >&2
  exit 1
fi
if getent passwd "$username" >/dev/null; then
  echo "error: $username already exists; review its SSH and sudo configuration before rerunning" >&2
  exit 1
fi
for path in "$receiver" "$sudoers" "$release_key"; do
  if [[ -e "$path" || -L "$path" ]]; then
    echo "error: refusing to overwrite existing publisher state: $path" >&2
    exit 1
  fi
done

read -r key_type key_data _ < "$ssh_key_input"
if [[ "$key_type" != ssh-ed25519 || ! "$key_data" =~ ^[A-Za-z0-9+/]+={0,2}$ ]]; then
  echo "error: publisher access must use one plain Ed25519 public key line" >&2
  exit 1
fi
temp_key="$(mktemp)"
created_user=0
created_receiver=0
created_sudoers=0
created_release_key=0
created_release_key_dir=0
committed=0
rollback() {
  status=$?
  trap - EXIT
  if [[ "$committed" -ne 1 && "$status" -ne 0 ]]; then
    [[ "$created_release_key" -eq 1 ]] && rm -f -- "$release_key"
    [[ "$created_release_key_dir" -eq 1 ]] && rmdir -- "$release_key_dir" 2>/dev/null || true
    [[ "$created_sudoers" -eq 1 ]] && rm -f -- "$sudoers"
    [[ "$created_receiver" -eq 1 ]] && rm -f -- "$receiver"
    if [[ "$created_user" -eq 1 ]]; then
      userdel --remove "$username" >/dev/null 2>&1 || true
    fi
  fi
  rm -f -- "$temp_key"
  exit "$status"
}
trap rollback EXIT
printf '%s %s\n' "$key_type" "$key_data" > "$temp_key"
ssh-keygen -lf "$temp_key" >/dev/null
openssl pkey -pubin -in "$release_key_input" -noout >/dev/null
release_key_bytes="$(openssl pkey -pubin -in "$release_key_input" -outform DER | tail -c 32 | od -An -v -tx1 | tr -d ' \n')"
if [[ "$release_key_bytes" != 1e3ab22c2cf6cb7c67cf92ca5a9e242e8fc697c15ed04589da4cbd7ed1842a43 ]]; then
  echo "error: release public key does not match the key pinned by Vapor Installer" >&2
  exit 1
fi

useradd --system --create-home --home-dir "$home" --shell /bin/sh "$username"
created_user=1
install -d -o "$username" -g "$username" -m 0700 "$home/.ssh"
printf 'restrict,command="/usr/bin/sudo -n --preserve-env=SSH_ORIGINAL_COMMAND %s" %s %s\n' "$receiver" "$key_type" "$key_data" \
  > "$home/.ssh/authorized_keys"
chown "$username:$username" "$home/.ssh/authorized_keys"
chmod 0600 "$home/.ssh/authorized_keys"

created_receiver=1
install -o root -g root -m 0755 "$receiver_source" "$receiver"
created_sudoers=1
printf 'Defaults:%s env_keep += "SSH_ORIGINAL_COMMAND"\n%s ALL=(root) NOPASSWD: %s\n' \
  "$username" "$username" "$receiver" > "$sudoers"
chmod 0440 "$sudoers"
visudo -cf "$sudoers"

if [[ ! -d "$release_key_dir" ]]; then
  created_release_key_dir=1
fi
install -d -o root -g root -m 0755 "$release_key_dir"
created_release_key=1
install -o root -g root -m 0644 "$release_key_input" "$release_key"
committed=1
echo "Installed the restricted Vapor release publisher. Configure the matching private SSH key, pinned SSH host key, hostname, and username in the vapor-stable-release GitHub environment."
