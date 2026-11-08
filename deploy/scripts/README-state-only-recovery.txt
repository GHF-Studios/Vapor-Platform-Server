Vapor Platform state-only recovery

This folder is a source-pinned recovery kit. It contains recovery tooling, not
server configuration, credentials, or a copy of the Platform source tree.
The matching State-Snapshots archive contains allowlisted service state and the
exact Platform source revision needed to rebuild the host.

1. Provision a fresh Ubuntu x86_64 VPS and connect as root.
2. Copy the selected .tar.gz archive and its .sha256 sidecar to that host.
3. Copy this complete kit folder to the host.
4. From this folder, run:

   bash ./recover-state-only-host.sh --bundle /path/to/vapor-platform-state.tar.gz --yes

The recovery kit and archive are checked before installing packages or changing
application paths. The first run creates a root-owned, secret-free marker pinned
to the archive digest and exact Platform revision, then rebuilds the matching
checkout and imports only the archive's state. Provider credentials are entered
without echoing. If setup is interrupted, rerun the same command with `--resume`
and the same archive; the script validates the marker and staged checkout before
continuing. If power fails during the initial marker write, rerun the first-run
command: the helper removes only its validated atomic-write temporary file. If
two recovery processes overlap, the second exits without changing the host. It
refuses to adopt unrelated host data.

After restore, complete browser sign-in and verify the owner dashboard, then run:

   bash ./recover-state-only-host.sh --bundle /path/to/vapor-platform-state.tar.gz --yes --resume --confirm-provider-checks

The marker is removed only after readiness checks and this explicit owner
confirmation. Keep the USB archive and kit until this final step succeeds.
