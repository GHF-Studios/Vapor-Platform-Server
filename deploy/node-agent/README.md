# Vapor node telemetry reporter

This standalone Linux utility sends only CPU, memory, uptime, one-minute load,
sample time, and reporter version to an enrolled Platform node. Its credential
cannot enroll nodes, read server data, or invoke commands.

## Build

Build for the Linux architecture of the worker:

```sh
cargo build --release --bin vapor-node-agent
```

Install `target/release/vapor-node-agent` as
`/usr/local/bin/vapor-node-agent` with root ownership and mode `0755`.

## Configure

Create the unprivileged service account and protected configuration directory:

```sh
sudo useradd --system --user-group --no-create-home --shell /usr/sbin/nologin vapor-node-agent
sudo install -d -o root -g vapor-node-agent -m 0750 /etc/vapor
```

Create `/etc/vapor/node-agent.toml` as `root:vapor-node-agent`, mode `0640`:

```toml
platform_origin = "https://vapor.ghf-studios.site"
node_id = "<node ID from the one-time enrollment response>"
reporter_credential = "<one-time reporter credential>"
```

Use the credential only in this file. Do not put it in shell arguments, shell
history, a unit file, or an environment variable. The reporter checks that the
file is regular, root-owned, group-readable by its service account, and not
readable or writable by other users.

## Validate and run

Run one report in the foreground to validate the connection:

```sh
sudo -u vapor-node-agent /usr/local/bin/vapor-node-agent --config /etc/vapor/node-agent.toml --once
```

After that succeeds, install the included unit as
`/etc/systemd/system/vapor-node-agent.service`, then enable it:

```sh
sudo systemctl daemon-reload
sudo systemctl enable --now vapor-node-agent.service
```

The service sends one report every 30 seconds, uses normal HTTPS certificate
validation, refuses redirects, and has bounded connection/request timeouts.
Revoke the node in Platform to stop accepting reports; removing the unit and
configuration from the host is a separate cleanup step.
