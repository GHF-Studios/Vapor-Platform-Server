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

The recovery validates the archive before installing packages or changing
application paths. It rebuilds the pinned Platform checkout, asks for public and
admin domains, and prompts for provider credentials without echoing them. It
then imports only the archive's state. Host setup is recreated from source.

This is an initial one-shot fresh-host workflow. If it stops after setup begins,
inspect the host and reinstall/clean it before starting again. Keep the existing
USB archive and kit until a fresh-host restore has been completed and verified.
