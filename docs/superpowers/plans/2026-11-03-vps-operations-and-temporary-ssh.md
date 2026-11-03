# VPS Operations and Temporary SSH Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Recover the VPS onto the approved Platform revision, replace periodic deploy polling with event-triggered approved operations, and add human-approved, expiring SSH grants in the existing operations dashboard.

**Rollout context:** This is the operational spine for the larger Vapor rollout,
not a replacement for the target-snapshot work. The Build Cache service,
Platform composition, path routing, shared admin shell, independent service
units, and composed state export/import already exist in source. The VPS-hosted
Linux/Windows VM builder remains a separate follow-on phase below.

**Architecture:** Platform owns request UX and audit records, while a narrow root-owned `vapor-platform-ops` helper performs only enumerated host operations. The helper independently checks the Identity root session and never accepts arbitrary commands. Deploy approvals trigger one request-specific systemd oneshot; SSH grants use per-grant accounts and persistent one-shot expiry units.

**Tech Stack:** Rust (Axum, Tokio, SQLx/SQLite, Serde), existing Identity automation credentials, OpenSSH, sudoers, systemd, Bash deployment scripts.

**Spec:** `docs/superpowers/specs/2026-11-03-temporary-ssh-access-design.md`

## Global Constraints

- Do not commit secrets, private keys, root passwords, tokens, or VPS-specific credentials.
- Keep Platform deployment/orchestration ownership at the root repo; Identity credential-scope logic remains in its submodule.
- Only server-defined capabilities may be requested or granted; never accept shell strings, executable paths, or sudoers text.
- A grant requires a fresh Identity root authorization check; a Platform SQLite `approved` value alone is not root authorization.
- The default SSH login is a distinct temporary account; leave root SSH and existing administrator access unchanged.
- Hard maximum SSH grant duration is eight hours; the request describes intent, while the root operator selects the actual grant from the server allowlist and may reduce or expand scope and duration.
- Keep the state-export backup timer. Remove the five-minute deploy poller.
- Preserve and report the existing VPS state before cutover; abort on any source or service state that is not explicitly recognized.

## Review Focus

- Invalid or unsupported public key: reject before creating any account or file.
- Unknown capability, oversized duration, or malformed request ID: reject before any privileged action.
- Helper unavailable or Identity unavailable: do not grant access or claim deployment dispatch succeeded.
- Duplicate approval, dispatch, revoke, or expiry event: operations must be idempotent and must not create extra accounts or units.
- Expiry during reboot or Platform downtime: persistent systemd expiry must remove access on the next boot, even if the dashboard is unavailable.
- Cleanup must serialize concurrent revoke/expiry operations, must not report success or delete timer files until an existing expiry timer is confirmed inactive and disabled, and must treat missing units from partial provisioning as an ordinary cleanup case.
- Worker crash/reboot after a deploy claim: never leave `executing` requests silently stuck and never automatically replay a possibly partial production deploy. Reconciliation must first confirm the request-specific systemd unit is inactive, then record a terminal failure that requires a new reviewed request.

---

### Task 1: Add requester-initiated browser authorization

**Files:**
- Modify: `Vapor-Identity-Server/src/automation_credentials.rs`
- Modify: `Vapor-Identity-Server/src/db.rs`
- Modify: `Vapor-Identity-Server/src/lib.rs`
- Modify: `Vapor-Identity-Server/README.md`
- Create: `Vapor-Identity-Server/src/cli_authorizations.rs`

**Interfaces:**
- CLI starts an Identity device authorization containing its local SSH key fingerprint, purpose, requested capabilities, and duration.
- A signed-in browser profile confirms or denies request submission after reviewing those exact values.
- After confirmation, Identity issues a three-minute, single-use credential bound to the fingerprint and digest of the canonical request fields. Platform's existing verifier carries these bindings and refuses any changed request.
- The generic automation-credential UI must reject `platform:ssh-access:request`; only this browser-confirmed flow can mint it. This credential submits a request only and cannot approve or activate a grant.

- [x] Add requester start, browser confirm/deny, and CLI poll endpoints with short-lived hashed codes and audit records.
- [x] Add one-time credential issuance and consume-once verification with key and request bindings.
- [x] Exclude the SSH scope from generic credential creation and document that it can submit requests only.
- [x] Commit and push the Identity submodule update; record its commit for the Platform pin.

### Task 2: Add SSH request and grant records

**Files:**
- Create: `src/ssh_access.rs`
- Modify: `src/main.rs`
- Modify: `src/deploy_requests.rs` only to share existing root-authorization/session extraction helpers

**Interfaces:**
- `POST /api/ssh-access/requests` accepts `{ public_key, purpose, requested_capabilities, requested_duration_seconds }` and a one-time Identity credential bound to the exact public-key fingerprint and request tuple.
- `GET /api/admin/ssh-access/requests` lists root-visible requests with computed OpenSSH fingerprints.
- `POST /api/admin/ssh-access/requests/{id}/approve` accepts expected version, confirmation, any server-allowed granted capability set, and a duration up to eight hours.
- `POST /api/admin/ssh-access/grants/{id}/revoke` revokes an active grant immediately.
- Allowed capabilities: `vapor_deploy`, `vapor_service_control`, and `host_admin`; shell is implicit and `host_admin` is exclusive. Restore is deferred until it has a request-bound safe interface.
- Allowed key type for v1: `ssh-ed25519`; maximum request duration: 28,800 seconds.

- [x] Add SQLite tables for requests, grant state, and audit transitions. Store no private key or session token.
- [x] Validate input size, key syntax, supported key type, fingerprint, purpose length, duration, and capability set before insert.
- [x] Authenticate request creation using Identity's one-time verifier and compare both bindings; authenticate listing/approval/revocation with the existing root session check.
- [x] Use compare-and-update request versions and no-store JSON responses.

### Task 3: Implement the fixed root operation helper

**Files:**
- Create: `src/bin/vapor_platform_ops.rs`
- Modify: `Cargo.toml` if Tokio's `process` feature is needed by Platform

**Interfaces:**
- Platform invokes `/usr/bin/sudo -n /usr/local/libexec/vapor-platform-ops` with one bounded JSON object on stdin and no command-line arguments.
- Helper operations are a closed enum: `dispatch_deploy`, `reconcile_deploy`, `grant_ssh`, `revoke_ssh`, `expire_ssh`.
- Every user-initiated operation includes the current Identity session token, which the helper verifies directly against the loopback Identity authorization endpoint for the `root` role.
- The helper returns bounded JSON `{ ok, operation_id, state, detail }`; it never logs keys or authorization tokens.

- [x] Add root-mode, request-size, schema, identifier, and allowlist validation.
- [x] Validate root Identity authorization independently of the Platform handler.
- [x] For SSH grants, create one account per grant, install only the reviewed key, add only fixed root-owned sudo wrappers, and persist the grant's username/fingerprint/expiry.
- [x] For SSH revoke/expiry, remove the managed key and sudo policy, terminate that account's sessions, and remove the account; repeat calls safely.
- [x] Confirm an existing expiry timer is stopped and disabled before deleting its unit files; report systemd state or inspection failures instead of silently leaving an orphan timer.
- [x] Serialize concurrent revoke/expiry cleanup with a root-owned per-grant lock under `/run/lock`.
- [x] For deploy dispatch, validate the exact approved request ID/version and start only its fixed systemd template unit with `--no-block`.
- [x] Add the exact sudoers command rule with argument denial; validate it with `visudo -cf` during install.
- [x] Cross-check the `x86_64-unknown-linux-gnu` target locally with Zig; this compiles the Unix-only helper and the locked dependency graph.
- [ ] Build the native Linux release on the VPS during cutover before installing the helper; the cross-target check does not replace that deployment build.

### Task 4: Replace the deploy timer with approval-triggered units

**Files:**
- Modify: `src/deploy_requests.rs`
- Modify: `src/admin_auth.rs`
- Modify: `deploy/scripts/deploy-request-queue.py`
- Modify: `deploy/scripts/run-approved-deploy.sh`
- Modify: `deploy/scripts/install-auto-deploy.sh` or replace it with `install-approved-operations.sh`
- Create: `deploy/systemd/vapor-deploy@.service`
- Remove from installation: `deploy/systemd/vapor-deploy.timer`

**Interfaces:**
- Approval invokes the helper with request ID/version after the root-confirmed database transition.
- The worker receives one UUID and claims only that approved request/version.
- `systemctl start --no-block vapor-deploy@<uuid>.service` is the sole deploy trigger; no periodic poller remains.
- Admin UI can retry a dispatch that failed before the worker claimed the request.

- [x] Make queue claim accept a specific request ID/version and preserve atomic claim semantics.
- [x] Install the oneshot template and remove/disable the old deploy timer idempotently.
- [x] Add direct dispatch and retry handling, with truthful queued/running/succeeded/failed state.
- [x] Keep the independent state-export timer unchanged.
- [x] Add root-authorized stale-execution reconciliation that checks the matching systemd unit is `inactive` or `failed` before marking a claimed request failed; do not add blind automatic retry.

### Task 5: Integrate the operator dashboard and install path

**Files:**
- Modify: `src/admin.rs`
- Modify: `deploy/scripts/deploy.sh`
- Create: `deploy/scripts/install-platform-ops.sh`
- Create: `deploy/scripts/remove-platform-ssh-access.sh`
- Modify: `Vapor-Identity-Server` submodule pin in the root repository

**Interfaces:**
- Existing Operations section gains Deploy Requests and SSH Access panels; no separate admin shell.
- SSH request rows show key fingerprint/type, purpose, requested/granted capabilities, duration, status, host, and expiry. The internal profile ID remains server-side for audit joins.
- Expiry timers use an absolute deadline and `Persistent=true`; the helper executes the same cleanup used by manual revoke.
- Bootstrap validates SSH configuration before reload and never edits existing root keys or firewall rules.

- [x] Add bounded polling/refresh only while the admin panel is open; use the existing same-origin session and explicit confirmation buttons.
- [x] Add capability selection from the fixed enum and a duration control capped at eight hours.
- [x] Install helper, sudoers rule, and dynamic per-grant expiry units with root ownership and strict modes.
- [x] Validate `sshd -t` before reload, preserve root SSH settings, and roll back the staged config on failure.
- [x] Include a clean uninstall that refuses active/incomplete grants and removes only this feature's SSH policy.
- [x] Add the new Identity submodule pin and retain the existing service-owned dashboard boundaries.

### Task 6: Repair and execute the guarded VPS cutover

**Files:**
- Create: `deploy/scripts/recover-and-cutover.sh`
- Modify: `deploy/scripts/install-caddy.sh` (already fixed in commit `8df8079906a04adbad513056d7796f0446b8ee42`)
- Modify: `deploy/scripts/health-check.sh` (already records exact failing command in that commit)

- [ ] Make preflight accept a clean checkout from the exact Platform origin at any previously known revision, then fetch and verify the exact current approved main commit; do not delete untracked VPS content.
- [ ] Preserve service enable/active states, config, Caddy config, state, and source checkouts before writes.
- [ ] In rollback, validate only services that existed before cutover; optional standalone Artifact/Build Cache units must not be reported as missing rollback failures.
- [ ] Upload a bounded, redacted run report automatically, without requiring VNC output copy.
- [ ] Keep the old source path and backup until the new dashboard and approval flow are confirmed working.
- [ ] Add the tracked recovery/cutover script named above; it is currently not present in `deploy/scripts/`.

### Task 7: Resume the signed target-snapshot builder lane after cutover

**Existing source:** `Vapor-Build-Cache-Server/`,
`docs/vapor-build-snapshot-contract.md`, and
`docs/superpowers/plans/2026-11-02-vapor-target-build-snapshots-and-builders.md`.

- [x] Compose Build Cache in Platform and expose its public snapshot API and
  root-only inventory panel.
- [ ] Confirm the live service reports current lock configuration and one
  available snapshot per supported target. Do not infer snapshot availability
  from service health alone.
- [ ] Probe the single VPS for virtualization support, storage/RAM/CPU budget,
  and Windows guest licensing. If the VPS cannot host both target VMs, report
  the blocker instead of silently using an external worker or owner PC.
- [ ] Build the shared VM lifecycle and operation ledger, then add the pinned
  Linux and Windows guests. The Linux target is
  `x86_64-unknown-linux-gnu`; Windows is
  `x86_64-pc-windows-gnullvm`.
- [ ] Publish only after the build cache validates the VPS-issued ticket,
  locked toolchain identities, signed manifest, archive digest, and target.

Keep these as one release sequence: recover the approval-controlled Platform
first, then inspect Build Cache availability, then implement the VM builders.
Do not let the migration disable state export/import or overwrite the current
snapshot pointer.

## Completion Criteria

- The VPS runs the approved Platform revision and Caddy still serves the public site.
- Deploy requests execute only after root approval and start immediately through a per-request unit; the deploy polling timer is absent/disabled.
- SSH requests display the exact key fingerprint and requested powers; root approval independently selects the actual grant within server policy and the eight-hour limit.
- The temporary account has only its granted capabilities, and explicit revoke plus natural expiry remove access and terminate sessions.
- Every operation reports an auditable outcome through the shared Operations dashboard/API; VNC is needed only for first bootstrap/recovery, not routine approvals.
