# Typed VPS SSH Operations

**Status:** Owner authorized implementation of the typed local-operator path  
**Date:** 2026-11-03

## Intent

Unblock the currently authorized developer machine without hand-editing SSH
configuration or `authorized_keys`, and provide a small, typed VPS operator
interface for inspecting, granting, changing, and revoking temporary SSH
access. The web dashboard remains the normal interface once repaired.

## Current problem

The CLI successfully submitted request
`fb9ef006-f6ad-420c-9ad7-31337b2b1e52`, but SSH rejected the key when tested.
The Operations page is not currently a dependable way to inspect or grant it.
The installed privileged helper already creates isolated accounts, applies
fixed capability wrappers, and installs persistent expiry timers; however, it
only accepts opaque JSON from the web service and requires an Identity root
session token.

## Approaches

1. **Edit SSH files directly.** Fast, but leaves untracked access state, bypasses
   request records and cleanup, and is specifically the workflow the operator
   wants to avoid.
2. **Repair the dashboard first.** Preserves the browser approval model, but
   depends on diagnosing the current page and delays access.
3. **Add a root-only typed VPS command (selected).** It acts only on existing
   Platform request records, uses the existing account provisioner and expiry
   mechanism, and records a distinct local-operator audit actor. It gives a
   short bootstrap path while the dashboard is repaired afterward.

## Contract

- A local command such as `vapor-platform-ops ssh show <request-id>` displays
  status, fingerprint, purpose, requested/granted capabilities, duration,
  expiry, and computed account name without exposing private material.
- `... ssh verify <request-id>` is read-only. For an active grant it checks the
  managed account, exact authorized-key match, expected valid sudo policy,
  installed/enabled expiry units, and future expiry. It returns an explicit
  inconsistent result when the database says active but host artifacts do not
  match; key bytes are never printed.
- `... ssh grant <request-id> --capability ... --duration ...` accepts only a
  pending, unexpired request, displays the exact key fingerprint and grant
  scope, and requires an explicit confirmation before provisioning. Pending
  requests expire after three days; the local command enforces the deadline
  both before prompting and in the atomic status transition.
- `... ssh revoke <request-id>` removes the managed account and terminates its
  sessions through the existing cleanup path.
- `... ssh reconfigure <request-id>` is read-only guidance. Grant scope and
  duration are immutable after approval; changing them requires revoking the
  current grant and submitting a new requester-authenticated request. This
  avoids mutating an authorization after its requester confirmed its key and
  requested scope.
- Every mutation is root-only, validates the request ID and stored key
  fingerprint, obeys the existing capability allowlist and eight-hour maximum,
  uses compare-and-update status/version transitions, and records whether the
  actor was an Identity profile or local VPS root operator.
- Direct grants require an interactive confirmation after displaying the exact
  fingerprint, requested scope, selected scope, and expiry; a non-interactive
  invocation cannot approve a grant. The default requested
  capabilities for the current bootstrap remain `vapor_deploy` and
  `vapor_service_control` for at most eight hours; do not add `host_admin`.
- No static key allowlist, private key, reusable bearer token, or permanent
  access is introduced. Existing expiry timers remain authoritative.
- The web dashboard continues to use its Identity-root-session path and shares
  the same underlying grant/revoke operations; this command is an operator
  bootstrap, not a replacement web authority.

## Goals and non-goals

- **Goal:** Use one typed operation to grant the already-authorized request,
  then test SSH and resume Platform work.
- **Goal:** Make later inspect/grant/reconfigure/revoke actions reproducible,
  auditable, bounded, and independent of manual file edits.
- **Non-goal:** General-purpose root shell access, arbitrary SSHD editing, or a
  second persistent authorization database.
- **Non-goal:** Redesigning every admin section in this change. After SSH works,
  the dashboard is a separate planned workstream.

## Implementation calls

- Keep the command surface in the existing Platform operations tool rather
  than adding a new server or a generic CLI framework.
- Store local-operator attribution as an explicit actor kind/label in the
  Platform SSH audit stream; do not forge an Identity profile ID.
- Reconfigure uses the existing grant record and account lifecycle. If safe
  in-place changes cannot be guaranteed, revoke and require a new request
  rather than leaving mixed key or privilege state.

## Acceptance

- The current pending request can be granted only after displaying and
  confirming its exact fingerprint, duration, and selected capabilities.
- The command reports the resulting account name, host, granted scope, and
  expiry; SSH accepts the dedicated local key.
- `ssh verify` reports a healthy active grant only when every required host
  artifact exists and matches the stored grant; otherwise it identifies the
  host-state mismatch and exits unsuccessfully without changing state.
- Revoke and expiry remove the managed account, key, sudo policy, timer, and
  active sessions; repeated cleanup is safe.
- Identity-approved web operations remain supported and audit attribution is
  unambiguous for both paths.
