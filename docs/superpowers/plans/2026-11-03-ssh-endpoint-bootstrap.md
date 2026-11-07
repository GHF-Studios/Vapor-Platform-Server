# Temporary SSH endpoint bootstrap plan

**Goal:** Make the browser-authorized CLI SSH request endpoint available on the
live VPS, then let the owner approve one requester-originated SSH request.

**Design:** `docs/superpowers/specs/2026-11-03-ssh-endpoint-bootstrap-design.md`.

## Phase 1: publish the exact reviewed source

1. Identity `main` now points to `0efc105be16df13c8d388496de5a32714a97b694`.
2. Platform `main` now points to `d7c1645a1ee6928b73f888ecc5deeb183d730a39`,
   pinning the Identity commit above. Composed CI passed. Deployment-ticket
   creation skipped because its request token or public-base variable is not
   configured; no ticket exists.

## Phase 2: prepare the VPS action

1. Review `target/vps-ssh-bootstrap.sh`; the same one-time payload is published
   on temporary branch `sshboot` so it can be downloaded from the VNC session.
2. It verifies the official origin, clean existing checkout, exact current
   `main` SHA, CI success, Identity gitlink, source verifier, free disk, and
   service/timer preconditions before writes.
3. It keeps timestamped copies of the prior checkout, service config, Atlas
   output, and exportable service state under root-only VPS backup paths. The
   report contains only revisions, status codes, and result labels. It is
   uploaded to paste.rs for retrieval and should be deleted after review.
4. It stages the exact `main` commit and invokes the fixed deploy script with
   the narrowly-scoped one-time queue bypass. The live request worker cannot
   install itself because the current Operations route is unavailable.
5. It checks local Platform health, the CLI authorization handler, and the
   public Operations route. On failure it attempts to restore the prior source,
   config, Atlas output, service state, SSH policy files, and services; the
   backup is retained and rollback failure is reported explicitly.
6. The script does not start or enable the retired deploy timer. The new
   Platform deployment installs the request-scoped worker and keeps that timer
   disabled.

## Phase 3: request and approve temporary SSH

1. Build/use the requester-side CLI and start `platform ssh request`.
2. Confirm the exact key fingerprint, purpose, requested capabilities, and
   duration in the signed-in Identity browser flow.
3. Confirm the request appears in Platform Operations.
4. The owner independently chooses and approves the actual server-allowed
   grant. Verify access expiry/revocation behavior before using the session.

## Authorization checkpoints

- Owner must authorize publishing the two feature revisions to their respective
  `main` branches.
- Owner must review the exact one-time VPS script before running it.
- Owner separately approves the eventual SSH grant in the dashboard.
- No VPS deployment, SSH policy change, or grant creation occurs while preparing
  this plan.

## Current handoff

- Payload branch: `GHF-Studios/Vapor-Platform-Server:sshboot` at
  `f71f3bc` (not merged to `main`).
- Raw payload:
  `https://raw.githubusercontent.com/GHF-Studios/Vapor-Platform-Server/sshboot/target/vps-ssh-bootstrap.sh`
- No server command has been run by the agent. The owner must inspect and run
  the single download-and-execute command from the VNC console.
