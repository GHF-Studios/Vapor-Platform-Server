# Automatic Vapor CLI SSH Authorization Plan

**Goal:** Let `vapor platform ssh request` submit a request as an authenticated Vapor profile without anyone copying SSH keys or bearer tokens.

**Architecture:** The CLI creates the SSH key locally, starts a one-time Identity authorization bound to its public-key fingerprint and request details, and opens the browser for the requester to confirm. Identity returns a three-minute, single-use request credential bound to that fingerprint and a digest of the canonical purpose, sorted capability set, and requested duration; Platform checks the entire binding. The CLI keeps the credential only in memory and submits the public key to Platform. A root operator then defines the actual grant from the server allowlist, independently of the requested scope/duration, with the eight-hour cap and explicit host-admin confirmation.

**Tech Stack:** Rust CLI, Axum Identity service, SQLx/SQLite, current Platform SSH request API and root-owned operations helper.

**Spec:** `docs/superpowers/specs/2026-11-03-temporary-ssh-access-design.md`.

## Global Constraints

- Never transmit or store the SSH private key on the server.
- Never issue the CLI a root/dashboard session or general-purpose credential.
- Request authorization is per-request, bound to the fingerprint and request fields, single-use, and expires after three minutes.
- Browser authorization authenticates the requester and request only; it never grants SSH access.
- Root approval chooses actual capabilities and duration from server policy; maximum duration remains eight hours.
- Keep all existing worktree edits and user files untouched.

## Review Focus

- Expired, denied, consumed, or unknown browser authorization cannot issue a request credential.
- Browser authorization requires an active Identity profile session and same-origin POST.
- Concurrent polls cannot receive multiple credentials; early polls are throttled and retried.
- Any changed fingerprint, purpose, capability set, or requested duration fails the binding check; Platform rejects the request.
- Non-HTTPS production origins and redirects remain rejected.

---

### Task 1: Identity one-time request authorization

**Files:** Modify `Vapor-Identity-Server/src/lib.rs`, `db.rs`, `automation_credentials.rs`; create `cli_authorizations.rs`.

- Create a short-lived device/user-code record showing fingerprint, purpose, capabilities, and requested duration in the signed-in browser.
- Approval associates the requesting profile. The first successful poll atomically creates and returns a three-minute, single-use credential with only `platform:ssh-access:request`, bound to the SSH fingerprint and canonical request fields.
- The Identity verifier consumes that credential atomically and includes the fingerprint and request digest bindings in its response.

### Task 2: Platform key binding and operator-selected grant

**Files:** Modify `src/deploy_requests.rs`, `src/ssh_access.rs`, `src/admin.rs`, and `src/bin/vapor_platform_ops.rs`.

- Pass both bindings through the Identity verification response and reject a submitted key or request that does not match them.
- Let the root operator select any server-allowed capability set and duration up to eight hours, including reducing or expanding the request. Keep `host_admin` exclusive and separately confirmed.
- Keep the root helper's allowlist, session revalidation, expiry, and exact fingerprint checks.

### Task 3: CLI browser flow and documentation

**Files:** Modify `Vapor-CLI/crates/vapor_cli/src/command/platform_ssh.rs`, `command/model.rs`, `Vapor-CLI/README.md`.

- Replace manual credential entry with one-time browser authorization, automatic polling, in-memory token redemption, and automatic request submission.
- Remove `credential set/status/clear`; keep key removal.
- Preserve request options, local private-key protection, HTTPS-origin checks, and the existing request response.

### Task 4: Validate and record

- Run scoped formatting and compiler checks for Identity, Platform, and Vapor-CLI.
- Do not add or run tests unless requested by the owner.
- Review the diff to ensure no unrelated worktree state is staged or changed; record exact verification results.
