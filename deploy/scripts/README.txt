Vapor Platform fresh-host recovery kit

This folder is copied from one exact recovery archive and is matched to the
source commit recorded by that archive. Keep the complete folder beside the
archive and its .sha256 file when moving recovery data to a fresh Ubuntu host.

1. Provision a fresh supported Ubuntu x86_64 host and restore SSH access.
2. Copy the selected vapor-platform-recovery-*.tar.gz archive, its matching
   .sha256 file, and this complete recovery-kit folder to the host.
3. From the folder containing recover-fresh-host.sh and
   recovery_bundle_metadata.py, run:

   sudo bash ./recover-fresh-host.sh --bundle ./vapor-platform-recovery-....tar.gz --yes

The recovery script verifies the archive and its pinned source before changing
Vapor state. It installs the exact recorded toolchain, builds the pinned
services, restores state and configuration, then leaves the site available
while provider checks await owner confirmation. Exit code 3 means those checks
are pending, not that the restore failed. After signing in with the configured
Steam and GitHub accounts and checking the owner dashboard, rerun:

   sudo bash ./recover-fresh-host.sh --bundle ./vapor-platform-recovery-....tar.gz --yes --resume --confirm-provider-checks

This kit is not a backup of the Strato account, the SSH private key, or source
hosting. Keep those recovery paths separate. The recovery archive is
intentionally unencrypted and contains server configuration credentials.
