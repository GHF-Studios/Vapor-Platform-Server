# VPS Operations and Temporary SSH Access Design

## Goal

Unify the current VPS deployment approval path with temporary SSH access in the
existing Vapor Platform operations dashboard. An authenticated person can
request short-lived SSH access; the root operator approves an exact public key,
server-defined privilege set, and duration. Both SSH access and approved
deployments use an explicit root-owned operation path rather than a periodic
approval poller.

## Previous deployment state (before this source change)

- Platform deploy requests are persisted in SQLite and require an Identity root
  session to approve.
- The prior source used a five-minute `vapor-deploy.timer`; it did not deploy
  unapproved requests. This source change replaces it with an event-triggered,
  per-request systemd service.
- The Platform service runs as the unprivileged `vapor` account, while SSH,
  account, sudoers, and systemd changes require root.
- The existing Admin dashboard shell and Identity root-session checks remain
  the single web entry point for host operations.
- The last cutover attempt stopped at preflight because the preserved checkout
  revision differed from the script's expected revision. It made no VPS
  changes. Caddy is active and the public site responds.

## Unified operation path

Platform HTTP handlers do not receive general root access. A small root-owned
`vapor-platform-ops` helper accepts a length-limited JSON request over a fixed
sudo command. It revalidates the supplied Identity root session itself and
implements only enumerated operations: dispatch one already-approved deploy
request; grant an approved SSH request; revoke a grant; and expire a grant.
It never accepts arbitrary executable names, shell strings, sudoers content,
or arbitrary systemd unit names. The prior five-minute deploy poller is removed
by the migration. Approval directly starts a request-specific oneshot systemd unit;
the unit records its result in the existing request history. Failed dispatches
remain visible and can be retried by a root operator.

## Operator flow

1. The requester starts in Vapor CLI, which creates an OpenSSH key locally and
   submits only its public-key fingerprint, purpose, requested capabilities,
   and requested duration to a one-time Identity authorization. The signed-in
   browser confirms the exact request. Identity then issues a single-use,
   three-minute request credential bound to the fingerprint and a digest of
   the canonical purpose, sorted capability set, and requested duration.
   Platform checks both bindings before accepting the public key. The CLI uses
   the credential immediately; private key material is never sent to the
   server, and no long-lived CLI credential is created.
2. The Platform dashboard displays the requester, key type and fingerprint,
   purpose, requested capabilities, and requested duration.
3. A root operator can reject the request or define the actual grant from the
   server's capability allowlist, adjusting scope and duration above or below
   what was requested, up to the server-wide eight-hour limit. The exact grant
   is shown before confirmation. `host_admin` remains exclusive and requires a
   separate explicit confirmation.
4. The operator receives the ephemeral SSH username, host, port, fingerprint,
   grant time, and expiration time. The user authenticates using their own
   private key.
5. The dashboard can revoke a grant at any time. On expiration or revocation,
   the server removes the key and temporary privilege, terminates that account's
   sessions, and deletes the ephemeral account. An audit record remains without
   retaining the public key after the grant ends.

## Access model

Requests choose from server-defined capabilities, never arbitrary commands or
sudoers text:

- `shell`: temporary shell, no sudo access.
- `vapor_deploy`: shell plus a fixed wrapper that can trigger only an already
  approved deploy request.
- `vapor_service_control`: shell plus fixed service status/restart operations
  for explicitly named Vapor units.
- `host_admin`: full sudo for the grant duration. This is explicitly marked
  high-risk and requires a separate confirmation in the approval UI.

The approver can grant a combination of `vapor_deploy` and
`vapor_service_control`; shell is implicit for every grant. `host_admin` is
exclusive. Restore is intentionally excluded until it has its own safe,
request-bound interface. The initial policy has a configurable hard maximum of
eight hours; the UI offers common durations up to that maximum. Requested
capabilities and duration are context for the operator, not authorization
ceilings; the server's allowlist and hard maximum govern the actual grant.

Each grant receives a separate temporary Unix account and separate
`authorized_keys` entry. Root SSH remains disabled/unchanged. Password login,
agent forwarding, X11 forwarding, and port forwarding are disabled for these
accounts. Scoped sudo rules name only root-owned wrappers with fixed operations;
the full-admin profile grants unrestricted sudo only on that temporary account
and only until cleanup.

## Privilege boundary

The Rust Platform process must not write `/etc/ssh`, create users, edit sudoers,
or own a root credential. A small root-owned access helper performs those
operations. The helper independently verifies the current Identity root
authorization for each grant/revoke call; it does not trust an `approved` field
or a row in the Platform-writable SQLite database as proof of authorization.
The helper is callable only through a fixed no-argument sudo rule with a
bounded JSON request on standard input. It recomputes the SSH key fingerprint
and compares it with the fingerprint shown to and approved by the root
operator, then checks the capability allowlist, status, version, and expiry
before making changes.

`host_admin` means unrestricted sudo and can disable its own expiry timer or
create lasting access. The dashboard warns about this before grant. Expiry
removes the temporary account and key but cannot enforce a deadline against
someone who was granted unrestricted root or undo changes made during that
access.

## Expiry and recovery

- Each grant gets a persistent one-shot systemd timer with an exact expiry,
  rather than a recurring access poller. The deployment worker is separately
  activated by each approved request and has no periodic timer.
- A timer activation invokes the root helper to revoke the key, remove that
  grant's sudoers file, terminate the temporary account's sessions, and delete
  the account. `Persistent=true` ensures an expiry missed during downtime is
  processed after boot.
- Explicit revoke runs the same helper immediately.
- If the dashboard or Platform service is unavailable, already-installed
  expiry timers still operate. If the root helper or timer installation fails,
  the grant is not reported as active.
- Expiry closes access; it cannot reverse persistent changes already made by a
  user who had root during the grant.
- The bootstrap installer validates the generated `sshd` configuration before
  reload, preserves existing SSH/root access, and leaves the VNC console usable
  as the recovery path.

## Audit and abuse controls

Store the profile authenticated by the browser confirmation, approving
profile, request/grant IDs, fingerprint, capabilities, reason, start/end
timestamps, state transitions, and failure details. Do not store private keys,
authentication tokens, or shell output. Require browser-authenticated
requesters, rate-limit open requests per profile and fingerprint, reject
weak/unsupported key types, and retain only the fingerprint after expiry.
Administrative views and every response use `no-store`.

## Delivery phases

1. Recover the Platform checkout from the failed cutover state without assuming
   a stale candidate revision; keep rollback truthful for optional services.
2. Replace periodic deploy polling with root-verified, event-triggered deploy
   dispatch and per-request result reporting.
3. Add SSH request/grant persistence and a section in the existing root-only
   operations dashboard, with fingerprint and capability confirmation.
4. Implement the root helper protocol and per-grant account/key/sudoers/timer
   lifecycle. Keep the helper's operation list fixed and the SSH configuration
   change isolated to temporary accounts.
5. Add guarded install/recovery behavior; validate `sshd -t` before any reload
   and preserve the existing login path.
6. Install via the short VNC bootstrap script, create a locally held client
   key, request and approve the minimum needed grant, verify SSH, revoke it, and
   verify natural expiry.

## Alternatives considered

- **Short-lived OpenSSH user certificates:** server-side validity prevents new
  authentication after expiry without per-key `authorized_keys` edits. This
  does not close an already-open shell by itself, and the user explicitly wants
  the granted key removed from the allowlist, so it is not the first version.
- **One shared operator account:** simpler, but combines concurrent grants and
  makes it harder to terminate only one grant's sessions. Use a distinct
  temporary account per grant.
