# Vapor node agent

This standalone Linux utility sends CPU, memory, uptime, one-minute load,
sample time, reporter version, and bounded network aggregates to an enrolled
Platform node. Network reports contain interface counters and TCP state counts;
they do not contain endpoints, peer addresses, process names, or packet data.
The optional separate executor credential can only claim and report
already-recorded typed service-restart operations for this node; it cannot
create operations or send commands.

## Obtain the tested package

The Platform CI run creates a `vapor-node-agent-linux-x86_64` artifact from the
same tested commit. Download `vapor-node-agent-linux-x86_64.tar.gz` and its
`.sha256` sidecar from that run, then verify and unpack it:

```sh
sha256sum -c vapor-node-agent-linux-x86_64.tar.gz.sha256
install_dir="$(mktemp -d)"
trap 'rm -rf -- "$install_dir"' EXIT
tar -xzf vapor-node-agent-linux-x86_64.tar.gz -C "$install_dir"
sudo install -o root -g root -m 0755 "$install_dir/vapor-node-agent" /usr/local/bin/vapor-node-agent
sudo install -o root -g root -m 0755 "$install_dir/vapor-node-ops" /usr/local/libexec/vapor-node-ops
sudo install -o root -g root -m 0644 "$install_dir/vapor-node-agent.service" /etc/systemd/system/vapor-node-agent.service
sudo install -o root -g root -m 0440 "$install_dir/vapor-node-agent.sudoers" /etc/sudoers.d/vapor-node-agent
sudo visudo -cf /etc/sudoers.d/vapor-node-agent
```

The archive contains the agent, restricted helper, sudoers rule, this README,
and the systemd unit. It is built for Linux x86_64. The temporary directory is
unique and removed when the shell exits.

### Build locally when the CI artifact is unavailable

On the worker, check out this repository and build with the locked dependency
set using its native Rust toolchain:

```sh
cargo build --locked --release --bin vapor_node_agent --bin vapor_node_ops
sudo install -o root -g root -m 0755 target/release/vapor_node_agent /usr/local/bin/vapor-node-agent
sudo install -o root -g root -m 0755 target/release/vapor_node_ops /usr/local/libexec/vapor-node-ops
sudo install -o root -g root -m 0644 deploy/node-agent/vapor-node-agent.service /etc/systemd/system/vapor-node-agent.service
sudo install -o root -g root -m 0440 deploy/node-agent/vapor-node-agent.sudoers /etc/sudoers.d/vapor-node-agent
sudo visudo -cf /etc/sudoers.d/vapor-node-agent
```

This builds for the worker's Linux architecture. The published CI artifact is
currently x86_64 only; use the local build path for another Linux architecture.

## Configure

Create the unprivileged service account and protected configuration directory:

```sh
sudo useradd --system --user-group --no-create-home --shell /usr/sbin/nologin vapor-node-agent
sudo install -d -o root -g vapor-node-agent -m 0750 /etc/vapor
```

The root-only **Servers** page can download the one-time TOML config immediately
after enrollment. Treat that download like a password: transfer it directly to
the enrolled host, install it with the ownership and mode below, and remove the
extra copy from the workstation. The page clears its copy when the enrollment
panel is closed; it cannot retrieve the credential again.

Create `/etc/vapor/node-agent.toml` as `root:vapor-node-agent`, mode `0640`:

```toml
platform_origin = "https://vapor.ghf-studios.site"
node_id = "<node ID from the one-time enrollment response>"
reporter_credential = "<one-time reporter credential>"
executor_credential = "<one-time separate executor credential>"
```

Use the credentials only in this file. Do not put them in shell arguments,
shell history, a unit file, or an environment variable. The agent checks that
the file is regular, root-owned, group-readable by its service account, and not
readable or writable by other users. Older reporter-only configs stay
telemetry-only when the optional executor credential is missing.

The systemd service remains unprivileged. Its sudo rule permits one fixed
root-owned helper, with no command-line arguments. The helper accepts a short,
expiring JSON action from stdin, checks the enrolled node ID, and maps a fixed
service ID to a fixed systemd unit. It rejects Platform, Caddy, host reboot,
shell, unit-name, and arbitrary argument requests. The executor polls over
outbound HTTPS every three seconds; telemetry continues independently every
30 seconds.

## Validate and run

Run one report in the foreground to validate the connection:

```sh
sudo -u vapor-node-agent /usr/local/bin/vapor-node-agent --config /etc/vapor/node-agent.toml --once
```

After that succeeds, enable the installed unit:

```sh
sudo systemctl daemon-reload
sudo systemctl enable --now vapor-node-agent.service
```

The service sends one report every 30 seconds, uses normal HTTPS certificate
validation, refuses redirects, and has bounded connection/request timeouts.
The current report schema is 3; reports from older agent builds are rejected.
Install the server and agent from the same release. It includes bounded interface/TCP aggregates
and current resource readings for nine fixed systemd units (`homepage`,
`docs`, `identity`, `diagnostics`, `registry`, `artifact`, `build-cache`,
`platform`, and `caddy`). Service readings come from cgroup v2; missing
controllers, stopped units, and unsupported cgroup layouts remain unavailable.
CPU is interval usage expressed as millicores (1000 means one fully occupied
core); memory is current usage and block I/O totals are cumulative since the
unit started. The agent does not enumerate arbitrary cgroups or invoke
`systemctl` while sampling. Platform stores only the latest service snapshot.

The root-only Server Diagnostics view retains aggregate, non-loopback traffic
samples for at most seven days in Platform state. Per-interface counters and
per-service resource values are held only as the latest sample. No peer
addresses, process names, packet contents, or player Diagnostics data are
involved.
Revoke the node in Platform to stop accepting reports; removing the unit and
configuration from the host is a separate cleanup step.
