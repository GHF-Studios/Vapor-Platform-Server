# SSH Request Entry Points Design

## Intent

Provide both a Vapor CLI path and a Platform Admin page for submitting a
temporary SSH access request. Both paths must use the existing Identity
confirmation and Platform request contract so convenience does not create a
second authorization model.

## Current behavior

- Identity exposes a short-lived SSH authorization start/poll flow. A signed-in
  profile confirms the exact fingerprint, purpose, requested capabilities, and
  duration.
- Identity returns a single-use credential bound to that request. Platform
  consumes it when creating the pending SSH request.
- A root operator separately chooses the actual grant in Admin. This design
  does not grant access or change the existing approval flow.
- Vapor CLI currently has no `platform ssh request` command. The Admin SSH page
  lists grants but does not create requests.

## Approaches

1. **Shared Identity flow in both entry points (selected):** CLI creates or
   selects a local Ed25519 identity and completes the browser-confirmed flow;
   Admin accepts a public key and completes that same flow in a second tab.
   Both submit the same strict payload and one-use credential to Platform.
2. **Admin-only direct creation:** simpler interaction, but would let an Admin
   cookie bypass the existing requester confirmation and request binding.
3. **CLI only:** preserves the current authorization model but leaves the
   requested Admin entry point absent.

## Invariants and scope

- Neither entry point sends or stores a private key on the server. Admin accepts
  only a public key. CLI private key material remains local under Vapor User
  Data; the CLI can also reference an existing local identity.
- Both paths show the exact request in the signed-in Identity confirmation and
  keep the one-use credential in memory only.
- Platform validates the same canonical request binding for both paths. A
  changed key, purpose, capability set, or duration is rejected.
- Request confirmation permits submission only. The existing root approval
  chooses the granted capability set and duration independently.
- Reuse the current API routes and request DTO; do not add an Admin-cookie
  creation endpoint, new credential scope, or changes to grant policy.
- Add `vapor platform ssh request` with purpose, requested capabilities, and
  duration options. The command should reuse the Vapor User Data identity by
  default and support selecting an existing local identity. Exact local key
  creation/storage mechanics remain an implementation call constrained by the
  platform's OpenSSH availability and file-permission model.
- Add a compact request form to `/admin/ssh-access`. It starts the existing
  Identity authorization, opens its verification URL in a separate tab, polls
  using the device code held in that page, and submits only after confirmation.
  Show pending/created/error states and link the result to the existing
  Approvals queue.

## Non-goals

- No auto-approval, direct grant creation, remote command execution, or
  change to capability allowlists, duration limits, root checks, or expiry.
- No private-key paste/upload, server-side private-key storage, or general
  automation credential UI changes.
- No production deployment or host key installation as part of this feature.

## Validation

- Check the CLI grammar and request serialization/validation with focused local
  verification.
- Verify the Admin form follows the existing start → browser confirmation →
  poll → one-use submission sequence and does not submit before confirmation.
- Run only checks through Vapor's managed toolchain. Keep production deployment
  and a live SSH grant as separate owner actions.
