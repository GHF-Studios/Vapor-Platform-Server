# Temporary SSH endpoint bootstrap design

## Problem

The requester-initiated SSH flow is implemented in the pushed Identity and
Platform feature branches, but the live Identity service returns 404 for its
new CLI authorization route. No SSH request exists yet. The Platform deploy
worker accepts only exact revisions reachable from `main`; the existing
`deploy.sh --bootstrap` is restricted to an empty first-install directory and
must not be used against the preserved live checkout. The reviewed source is
now published to `main` and composed CI passed, but the deploy-request workflow
skipped ticket creation because its request token or public-base variable is
not configured. The public Operations route was also previously observed as
404. The bootstrap is therefore the route to bring up the missing approval
surface, not a way to bypass an available ticket.

## Goal

Give the operator one short, reviewable action in the VPS console that installs
the exact authorized Platform revision with its pinned Identity revision,
preserving the current checkout, configuration, and service state for rollback.
After this endpoint is live, the requester can use the updated local CLI and
the normal browser-authorized SSH request flow.

## Proposed sequence

1. Identity `main` is now at `0efc105be16df13c8d388496de5a32714a97b694`.
2. Platform `main` is now at `d7c1645a1ee6928b73f888ecc5deeb183d730a39`,
   pinning that Identity commit. Composed CI passed; the deployment-request
   workflow skipped ticket creation because the request token or public-base
   variable is absent.
3. Prepare an operator-run VPS bootstrap script. Before changing
   anything it verifies the expected checkout path and official origin, exact
   current/candidate revisions, clean tracked checkout, and required services.
   It records a timestamped backup of the current Git checkout reference,
   existing Platform/Identity config files and service state; it never copies
   secrets into the repository or its sanitized status report.
4. Fetch the `main` revision, verify the root tree and submodule pins using the
   existing source verifier, check out the exact commit and initialize pinned
   submodules.
5. Run the fixed `deploy.sh --approved <sha>` against that exact checkout using
   the existing one-time bootstrap environment override. This is necessary
   because the current public deployment/admin routes are not live, so the
   normal server-owned request worker cannot yet be used to install itself.
   The override is limited to this explicit operator-run migration; it is not
   enabled in a service, timer, or future deployment.
6. Confirm Identity and Platform health plus the new CLI authorization route.
   If deployment fails, restore the prior checkout revision and rerun the fixed
   deploy script for that revision; retain the backup and print one concise
   result. The script must stop and report a rollback failure rather than claim
   recovery if the old revision does not restore health.
7. Once the endpoint is live, install/use the already-built CLI, submit the
   requester-owned SSH public key, and let the operator approve the request in
   the dashboard. This is a separate human grant decision.

## Constraints and safety boundaries

- No direct feature-branch deployment; candidate commits must be on `main` and
  satisfy the current source verifier. The one-time queue bypass is run only by
  the owner in the VPS console to bootstrap the unavailable request system.
- No server action is performed by this design or its local preparation.
- Preserve the existing `/opt/vapor-platform-server` checkout and state. Never
  treat it as an empty bootstrap directory or replace it recursively.
- Do not re-enable the retired periodic deploy timer.
- Do not write credentials, private keys, or VPS-specific secrets into Git or
  the temporary bootstrap artifact.
- The bootstrap does not create or approve an SSH grant. The dashboard grant
  remains a separate explicit human decision after the endpoint is live.

## Operator handoff

The fast-forwards are complete. The one-time payload is at
`target/vps-ssh-bootstrap.sh`. It is pinned to the two published revisions,
requires successful Platform CI for the exact commit, and checks that `main`
still matches. It keeps config/state/backups on the VPS and uploads only a
brief sanitized status report to paste.rs; the operator can delete that report
after review. Paste.rs does not advertise a short expiry, so this is not
treated as an expiring/private paste. The queue bypass is limited to this
manual bootstrap; later deployments must use the normal request/approval
worker.
