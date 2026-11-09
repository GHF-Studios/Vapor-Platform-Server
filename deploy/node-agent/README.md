# Vapor node telemetry reporter

This standalone Linux utility sends only CPU, memory, uptime, one-minute load,
sample time, and reporter version to an enrolled Platform node. Its credential
cannot enroll nodes, read server data, or invoke commands.

## Obtain the tested package

The Platform CI run creates a `vapor-node-agent-linux-x86_64` artifact from the
same tested commit. Download `vapor-node-agent-linux-x86_64.tar.gz` and its
`.sha256` sidecar from that run, then verify and unpack it:

```sh
sha256sum -c vapor-node-agent-linux-x86_64.tar.gz.sha256
mkdir -m 0700 /tmp/vapor-node-agent-install
tar -xzf vapor-node-agent-linux-x86_64.tar.gz -C /tmp/vapor-node-agent-install
```

The archive contains the reporter binary, this README, and the systemd unit. It
is built for Linux x86_64. Install the binary and unit with root ownership:

```sh
sudo install -o root -g root -m 0755 /tmp/vapor-node-agent-install/vapor-node-agent /usr/local/bin/vapor-node-agent
sudo install -o root -g root -m 0644 /tmp/vapor-node-agent-install/vapor-node-agent.service /etc/systemd/system/vapor-node-agent.service
```

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

After that succeeds, enable the installed unit:

```sh
sudo systemctl daemon-reload
sudo systemctl enable --now vapor-node-agent.service
```

The service sends one report every 30 seconds, uses normal HTTPS certificate
validation, refuses redirects, and has bounded connection/request timeouts.
Revoke the node in Platform to stop accepting reports; removing the unit and
configuration from the host is a separate cleanup step.
