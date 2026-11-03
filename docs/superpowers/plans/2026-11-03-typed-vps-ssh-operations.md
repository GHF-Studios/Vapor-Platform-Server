# Typed VPS SSH Operations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a local VPS root operator inspect, grant, revoke, and safely reconfigure temporary SSH access without editing SSH files or pretending to be an Identity user.

**Architecture:** Extend the existing `vapor_platform_ops` root helper with a closed, human-readable `ssh` command surface. Reuse its account, capability-wrapper, and persistent expiry implementation; record local operator actions distinctly in the existing audit stream. The web Identity-root-session path remains unchanged and uses the same grant/revoke state transitions.

**Tech Stack:** Rust, Axum Platform SQLite schema, SQLx, OpenSSH account/key management, systemd expiry units, existing fixed sudo wrappers.

**Spec:** `docs/superpowers/specs/2026-11-03-typed-vps-ssh-operations-design.md`

## Global Constraints

- Require root for every typed `ssh` command; reject arbitrary commands and shell input.
- Never store, display, or transmit a private key, session token, or reusable bearer credential.
- Grant only existing pending request records and only server-allowlisted capabilities for at most eight hours.
- Display exact fingerprint, purpose, requested/granted capabilities, duration, and expiry before grant confirmation.
- Use an interactive confirmation; non-interactive grant and revoke calls fail closed.
- Reuse existing SSH account, sudo wrapper, expiry timer, lock, and cleanup code.
- Attribute local actions to a `local_operator` actor, never a forged Identity profile.
- Treat reconfiguration as revoke plus a new requester-supplied request if a safe in-place change cannot be guaranteed.
- Do not execute a grant on the live VPS as part of code implementation; installation and a grant are separate operational steps.

## Review Focus

- Malformed IDs, duplicate or unknown flags, invalid capability sets, and durations outside 1–28,800 seconds fail before any state or host mutation.
- Grant races between two operators produce one state transition and at most one managed account.
- Host-admin scope is exclusive and receives an additional explicit warning/confirmation.
- Failed provisioning remains visible as `failed` and can be safely cleaned up; it must never be shown as active.
- Revoke and natural expiry can race safely, stop/disable their timer, terminate the account, and remain idempotent.

## File Map

- Modify: `src/bin/vapor_platform_ops.rs` — root-only typed command dispatch and lifecycle orchestration.
- Modify: `src/ssh_access.rs` — additive SQLite migration and Identity actor defaults for audit events.
- Create: `src/ssh_cli.rs` — pure closed command parser and validation shared by the helper.
- Create: `tests/ssh_cli.rs` — parser and validation behavior tests.
- Modify: `deploy/scripts/install-platform-ops.sh` — install/permission/sudoers checks only if the typed command needs a separate command invocation rule.
- Modify: `docs/superpowers/specs/2026-11-03-typed-vps-ssh-operations-design.md` — only to record owner-approved contract changes.

## Task 1: Add the closed SSH command parser

**Interfaces:**
- `parse_ssh_args(args: &[String]) -> Result<SshCommand, CliParseError>` accepts only `list`, `show <uuid>`, `grant <uuid> --capability <value>... --duration <duration>`, `revoke <uuid>`, and `reconfigure <uuid>`.
- `SshCommand` carries validated UUID, capability vector, and duration seconds. It accepts `1..=28800` seconds and human-readable `s`, `m`, or `h` suffixes.
- `reconfigure` returns guidance to revoke and submit a new request; it never mutates an active grant in place.

- [x] Write parser tests for valid list/show/grant/revoke/reconfigure commands.
- [x] Write failing tests for invalid UUID, duplicate/unknown flags, duplicate/unknown capabilities, conflicting `host_admin`, and invalid duration.
- [x] Run the focused parser tests and confirm they fail for the expected missing parser behavior.
- [x] Implement the parser in `src/ssh_cli.rs`; reject every unrecognized command and trailing argument.
- [x] Run parser tests; all accepted shapes parse to exact expected values and invalid shapes fail.

## Task 2: Add auditable local-operator actors

**Interfaces:**
- Extend `platform_ssh_access_requests` with `decided_by_actor_kind TEXT NOT NULL DEFAULT 'identity_profile'` and nullable `decided_by_actor_label TEXT`.
- Extend `platform_ssh_access_events` with matching actor-kind and actor-label fields.
- Existing Identity request creation remains attributed to `identity_profile`; local root commands use `local_operator` and the literal actor label `local-root`.
- Migration is additive and safe to run on every startup; status-change triggers are replaced to copy the new actor fields.

- [x] Add migration code that detects existing columns before `ALTER TABLE` and safely upgrades the existing SQLite database.
- [x] Recreate the audit triggers to preserve existing Identity attribution and record typed local operator attribution.
- [x] Verify repeated initialization preserves rows, trigger behavior, and existing Identity profile attribution.

## Task 3: Implement read-only request inspection

**Interfaces:**
- `ssh list` displays a bounded table of request ID, status, fingerprint, purpose, requested/granted scope, and expiry.
- `ssh show <uuid>` displays one request’s full safe summary; it never returns the public key bytes or private material.
- Both commands require root and open the configured Platform DB read-only.

- [ ] Add tests for status rendering, missing IDs, and redaction of the stored public key.
- [x] Implement list/show queries and deterministic concise output.
- [ ] Run the read-only CLI checks against a temporary SQLite fixture; confirm no DB rows or host files are modified.

## Task 4: Implement grant, revoke, and reconfigure guidance

**Interfaces:**
- `ssh grant <uuid> --capability ... --duration ...` reads only a `pending` request, prints key fingerprint and the complete actual grant, then requires interactive `YES` confirmation. `host_admin` requires typing `UNRESTRICTED ROOT` after the normal prompt.
- Grant uses compare-and-update on the current request version, marks `provisioning`, writes actor fields and selected scope, then calls the existing `grant_ssh` account/timer implementation. It becomes `active` only after provisioning succeeds.
- On provisioning failure, mark `failed`, preserve a concise error, and allow the existing idempotent cleanup path.
- `ssh revoke <uuid>` prints current account/scope/expiry and requires interactive confirmation before using the existing serialized cleanup path; only successful cleanup becomes `revoked`.
- `ssh reconfigure <uuid>` explains the safe revoke-and-new-request workflow and performs no mutation.

- [ ] Add state-transition tests for pending-to-provisioning-to-active and provisioning-to-failed behavior.
- [ ] Add tests proving unsupported scope, absent confirmation, non-TTY use, and stale version fail before privileged side effects.
- [x] Implement local grant/revoke orchestration and set local actor fields in each transition.
- [x] Ensure output never prints raw public keys, secrets, or unrestricted helper detail.
- [x] Enforce the shared three-day pending-request expiry before grant confirmation and again in the atomic transition, so an old request cannot be provisioned through the local path.
- [ ] Run Linux helper state-transition tests; verify one active grant is created only after fingerprint confirmation and failures never produce `active`.

## Task 5: Document and package the typed operator surface

**Files:** `README.md`, `deploy/scripts/install-platform-ops.sh`, the existing helper unit/sudoers setup.

- [x] Document the exact short `ssh list/show/grant/revoke` syntax, duration suffixes, allowed capabilities, root requirement, and reconfigure-by-replacement behavior in `deploy/README.md`.
- [x] Inspect the installer’s root ownership/modes and its no-argument-only `vapor` sudo rule; the typed operator command remains root-invoked and does not broaden the service account rule.
- [ ] Build the Linux release helper and verify it rejects arguments on the legacy JSON path while accepting only the typed `ssh` subcommands.
- [ ] Confirm installation instructions do not edit `authorized_keys` manually or expose a private key.

## Completion Criteria

- The current request can be inspected locally with no secret disclosure.
- The operator can grant the requested `vapor_deploy` and `vapor_service_control` scope for no more than eight hours after viewing the exact key and confirming interactively.
- Local root actions appear distinctly in audit history; Identity actions remain attributed to Identity profiles.
- Revoke and expiry remove the dedicated account, key, sudo policy, timer, and sessions; repeated cleanup is safe.
- Reconfiguration cannot silently retain old privilege: it requires revoke and a new request.
- The existing web-based flow still works and can inspect the same request records.
