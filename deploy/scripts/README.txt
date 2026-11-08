Vapor Platform fresh-host recovery kit

This folder is copied from one exact recovery archive and is matched to the
source commit recorded by that archive. Keep the complete folder beside the
archive and its .sha256 file when moving recovery data to a fresh Ubuntu host.

1. Provision a fresh supported Ubuntu x86_64 host with Python 3.11 or newer,
   then restore SSH access. Ubuntu 24.04 or newer supplies this preflight
   runtime by default.
2. Copy the selected vapor-platform-recovery-*.tar.gz archive, its matching
   .sha256 file, and this complete recovery-kit folder to the host. The
   sidecar must contain one SHA-256 entry naming that exact archive.
3. From the complete folder containing recover-fresh-host.sh,
   recovery_bundle_metadata.py, state-bundle-validate.py,
   deployed_source_manifest.py, and service_deploy_map.py, run:

   sudo bash ./recover-fresh-host.sh --bundle ./vapor-platform-recovery-....tar.gz --yes

The recovery script checks kit consistency and fully validates the archive
before package installation or Vapor state changes. Archive checksum
verification rejects sidecars that name a different file or contain extra
entries. Kit and archive checksums detect corruption but are not signatures;
only run a kit from your trusted, owner-held recovery copy. It then checks out the
exact pinned source and validates the archive again with that source version.
It installs the exact recorded toolchain, builds the pinned
services, restores state and configuration, then leaves the site available
while provider checks await owner confirmation. Exit code 3 means those checks
are pending, not that the restore failed. After signing in with the configured
Steam and GitHub accounts and checking the owner dashboard, rerun:

   sudo bash ./recover-fresh-host.sh --bundle ./vapor-platform-recovery-....tar.gz --yes --resume --confirm-provider-checks

This kit is not a backup of the Strato account, the SSH private key, or source
hosting. Keep those recovery paths separate. The recovery archive is
intentionally unencrypted and contains server configuration credentials.
