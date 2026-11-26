# SSH Request Entry Points Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Vapor CLI and Platform Admin entry points for submitting temporary SSH requests through the existing Identity confirmation and single-use Platform credential flow.

**Architecture:** CLI creates or selects a local Ed25519 identity under Vapor User Data and performs the Identity start/confirm/poll/submit sequence. Admin accepts a public key and performs the same sequence in the browser, holding the device code and credential only in page memory. Platform's existing root approval remains the only grant path.

**Tech Stack:** Rust/Clap CLI, reqwest over HTTPS, OpenSSH key generation, Axum/HTML/JavaScript Admin page, existing Identity and Platform HTTP APIs.

**Spec:** `docs/superpowers/specs/2026-11-26-ssh-request-entry-points-design.md`

## Global Constraints

- Never transmit or store an SSH private key on the server.
- The Admin page accepts only a public key.
- Both request paths show exact request details in Identity's signed-in confirmation and keep the one-use credential in memory only.
- Platform validates the existing fingerprint and canonical request binding.
- Request confirmation permits submission only; Platform root approval independently selects actual privileges and duration.
- Do not add an Admin-cookie creation endpoint, new credential scope, or grant-policy changes.
- Use the Vapor-managed Rust/Cargo toolchain for lockfile, build, and test work.

## Review Focus

- Unsupported/malformed public key: reject locally with a clear error and never create an Identity authorization.
- Identity authorization denied or expired: do not submit a Platform request.
- Changed public key or request tuple: Platform rejects the one-use credential binding.
- Browser pop-up blocked or Identity unavailable: keep the request recoverable or report a retryable failure without claiming submission.
- Network redirect, non-HTTPS origin, or unexpected API payload: fail closed without exposing the device code or credential.

---

### Task 1: Vapor CLI SSH request command

**Files:**
- Create: `Vapor-CLI/crates/vapor_cli/src/command/platform_ssh.rs`
- Modify: `Vapor-CLI/crates/vapor_cli/src/command/model.rs`
- Modify: `Vapor-CLI/crates/vapor_cli/src/command.rs`
- Modify: `Vapor-CLI/crates/vapor_cli/Cargo.toml`
- Modify: `Vapor-CLI/Cargo.toml`
- Modify: `Vapor-CLI/Cargo.lock`
- Modify: `Vapor-CLI/crates/vapor_cli/docs/commands.md`
- Modify: `Vapor-CLI/crates/vapor_cli/README.md`

**Interfaces:**
- Add `ShellCommand::Platform { command: PlatformCommand }` and
  `PlatformCommand::Ssh { command: PlatformSshCommand }`.
- Add `PlatformSshCommand::Request { purpose: String, capability: Vec<String>, duration_hours: u8, identity_file: Option<PathBuf> }`, with duration defaulting to one hour and server-accepted values limited to 1–8 hours.
- Implement `execute_platform_ssh_request(...) -> Result<(), String>` in the new command module.

- [x] Add parser tests for default duration, repeated capabilities, identity override, and rejection of duration outside 1–8 hours.
- [x] Run the Vapor-managed CLI test from the Vapor-CLI source root and verify the new helper tests fail before implementation.
- [x] Implement identity selection: default under the discovered installation's Vapor User Data `ssh/` directory; use OpenSSH `ssh-keygen` to create an Ed25519 pair if absent; reuse a complete pair without overwriting it; reject incomplete pairs; allow an explicit existing identity path.
- [x] Implement strict public-key/fingerprint validation and the HTTPS Identity flow: start authorization, open the verification URL with the platform browser opener, poll using the returned device code, and submit to Platform only after receiving the approved one-use credential. If the browser opener fails, retain manual URL confirmation and continue polling.
- [x] Keep the credential only in process memory, disable redirects, enforce HTTPS for the configured `VAPOR_PLATFORM_BASE_URL` (default `https://vapor.ghf-studios.site`), and redact credentials from all errors/output.
- [x] Add focused tests for malformed/unsupported SSH keys, pending/denied/expired authorization outcomes, invalid or redirected API responses, exact submitted request fields, fingerprint parsing, and identity-pair reuse.
- [x] Run the Vapor-managed CLI tests; 25 tests passed.
- [x] Document `vapor platform ssh request`, defaults, options, local key path, browser confirmation, and separate root approval in the CLI README and commands guide.

### Task 2: Admin SSH request form

**Files:**
- Modify: `src/admin.rs` (SSH page markup, request-flow JavaScript, and focused page tests)
- Create: `Workspace.vapor.toml` (so the Platform root server crate is a first-class Vapor-managed source workspace)

**Interfaces:**
- Add a request form under `/admin/ssh-access` with public-key, purpose, requested-capability, and requested-duration fields.
- Use existing routes `POST /api/identity/v1/cli/ssh-authorization/start`, `POST /api/identity/v1/cli/ssh-authorization/poll`, `GET /cli/ssh/authorize`, and `POST /api/ssh-access/requests`.
- On successful submission, show the request ID and link to `/admin/approvals#pending-ssh-approvals`.

- [x] Add focused Admin page tests requiring the form, public-key-only input, Identity confirmation route, and no direct request submission before an approved credential is returned.
- [x] Add the root `Workspace.vapor.toml` using the same pinned stable toolchain as the Identity service and a single project matching Cargo package `vapor_platform_server`; this allows Vapor to own the Platform root's test/build path.
- [x] Open the Platform root as the active Vapor source and run the Vapor-managed tests; verify the new Admin assertions fail before implementation.
- [x] Implement client-side Ed25519 public-key parsing and SHA-256 fingerprint calculation using the same OpenSSH public-key blob semantics as Platform.
- [x] Implement start → open authorization in a separate tab → poll → one-use request submission, with timeout, cancel, blocked-popup, duplicate-submit, and safe error states; test that request submission occurs only after approved poll response. Disable cancellation once submission starts and report uncertain network outcomes accurately.
- [x] Ensure device codes and credentials are not persisted, logged, rendered, or included in URLs except the Identity verification URL's user code.
- [x] Run the Vapor-managed Platform tests; 152 unit tests and 3 SSH CLI surface tests passed.

### Task 3: Integrated verification and issue evidence

**Files:**
- Modify: `Vapor-Client#3` issue evidence if the CLI/Admin path unblocks publisher access
- Modify: `.agents/workstreams/open/standalone-vapor-runtime.md` with implementation and validation evidence
- Modify: Vapor-Client `Vapor-CLI` gitlink after the CLI submodule branch is committed and pushed

- [x] Run Vapor-managed tests and builds from each changed source workspace (Vapor-CLI and Platform root); no raw Cargo commands were used.
- [x] Inspect the final diff for private-key/token exposure and run `git diff --check` in each changed repository.
- [x] Record code/build/test evidence separately from any live SSH request, grant, host operation, or stable-release publication; those external steps are not validated by local checks.
- [ ] Commit and push `Vapor-CLI`, then commit and push its updated gitlink in Vapor-Client, and separately commit/push Platform on its existing feature branch; preserve unrelated changes and the established dependency order.
