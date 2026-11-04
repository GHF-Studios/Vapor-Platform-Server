# Deploy Approval Stage Reporting Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` to implement this plan task-by-task.

**Goal:** Keep one-click deploy approval while making the saved decision and deployment dispatch outcomes explicit, durable, and diagnosable.

**Architecture:** Platform persists an approval before invoking the existing privileged dispatch helper. Add idempotent SQLite columns for dispatch attempts/outcome, return stage-coded bounded API results, and render the decision and deployment stages separately in the existing Operations panel. The request status remains the worker lifecycle; dispatch state describes only the launch attempt and never authorizes execution.

**Tech Stack:** Rust, Axum, SQLx/SQLite, existing systemd helper, browser JavaScript embedded in `src/admin.rs`.

**Spec:** `docs/superpowers/specs/2026-11-04-deploy-approval-stage-reporting-design.md`

## Global Constraints

- Keep the single **Approve deploy** action.
- Keep Identity root-session authorization, same-origin checks, expected-version checks, immutable revision pins, and fixed-helper-only privileged operations.
- A failed dispatch does not undo or misrepresent the durable approval.
- Do not return SQL, helper stderr, cookies, service tokens, or other sensitive internals to the browser.
- Never retry a request unless the existing approved status/version still matches; worker claim remains atomic and idempotent.
- Use an explicit repeatable migration for existing SQLite state; do not assume `CREATE TABLE IF NOT EXISTS` updates old tables.

## Review Focus

- Approval persistence failure: the response says the decision was not saved and the row stays pending; test DB error.
- Helper dispatch failure after approval: the row stays approved, persists a retryable failed/unknown dispatch stage, and test response states approval was saved.
- Helper timeout or lost response: represent uncertainty honestly and allow only the existing version-checked idempotent retry; test ambiguous result.
- Worker claims while dispatch result is being recorded: preserve `executing` or terminal request state and record the dispatch attempt without reverting lifecycle status; test race ordering.
- Existing database without new columns: migration adds defaults and is safe to run repeatedly; test upgrade and repeat.

---

### Task 1: Add deploy dispatch-state migration and persistence helpers

**Files:**
- Create: `src/deploy_request_schema.rs`
- Modify: `src/main.rs`
- Modify: `src/deploy_requests.rs`
- Test: `src/deploy_request_schema.rs`

**Interfaces:**
- `deploy_request_schema::ensure_dispatch_columns(&SqlitePool) -> Result<(), sqlx::Error>` adds `dispatch_state TEXT NOT NULL DEFAULT 'not_attempted'`, `dispatch_attempts INTEGER NOT NULL DEFAULT 0`, `dispatch_attempted_at_unix INTEGER`, and `dispatch_detail TEXT` using the existing `PRAGMA table_info` migration pattern.
- `deploy_requests::initialize()` runs the migration after creating the original table and indexes.
- `dispatch_state` is one of `not_attempted`, `dispatching`, `queued`, `failed`, or `unknown`; `dispatch_detail` is a stable safe message capped at 240 characters.
- A dispatch attempt becomes stale after 30 seconds, above the existing 10-second helper wait timeout.
- `retry_eligible` is true only for a still-approved exact version whose dispatch state is `not_attempted`, `failed`, or `unknown`, or whose `dispatching` attempt is stale.
- Dispatch-state updates never change the worker-owned request `status` or `version` lifecycle.

- [ ] **Step 1: Add failing migration tests**
  - Test an existing pre-migration `platform_deploy_requests` table gets the four columns with neutral defaults.
  - Test migration can run twice without changing existing request rows or failing.
  - Test fresh initialization receives the same schema.
- [ ] **Step 2: Run the focused migration tests and confirm failure**
  - Run: `cargo test --locked --manifest-path Cargo.toml deploy_request_schema`
  - Expected: missing migration function/columns.
- [ ] **Step 3: Implement idempotent column migration**
  - Follow `src/ssh_schema.rs` `ensure_column` conventions; keep table/column names static and migration statements owned by this module.
- [ ] **Step 4: Verify focused migration tests pass**
  - Run: `cargo test --locked --manifest-path Cargo.toml deploy_request_schema`

### Task 2: Make approval and dispatch responses stage-aware

**Files:**
- Modify: `src/deploy_requests.rs`
- Test: `src/deploy_requests.rs`
- Modify: `src/bin/vapor_platform_ops.rs` only if typed helper outcome classification is needed; preserve the fixed operation allowlist.

**Interfaces:**
- Deploy request summaries include `dispatch_state`, `dispatch_attempts`, `dispatch_attempted_at_unix`, and `dispatch_detail`.
- Deploy request summaries also include computed `retry_eligible`; it is true only when the exact request remains approved and no fresh dispatch is already running.
- Structured error response: `{ "error": { "code": string, "stage": string, "message": string, "decision_saved": bool, "retry_eligible": bool } }`.
- Structured success response includes `{ id, status, version, decision: { status: "approved", saved: true }, dispatch: { status: "queued" } }`.
- Internal dispatch outcomes distinguish helper rejection from an unknown result (for example, helper timeout/communication loss); internal details go to the service journal only.
- `begin_dispatch_attempt(pool, id, approved_version, now) -> Result<Option<i64>, sqlx::Error>` returns a monotonically increasing attempt number only when the row remains at the exact approved version and no fresh dispatch attempt is active.
- `finish_dispatch_attempt(pool, id, attempt, outcome, detail, now) -> Result<bool, sqlx::Error>` updates only that attempt and never changes request `status` or `version`.

- [ ] **Step 1: Add failing lifecycle/outcome tests**
  - `approval_write_error_is_reported_as_not_saved`: force an UPDATE error and assert a `decision_not_saved` response with status still pending.
  - `helper_rejection_keeps_saved_approval_and_records_retryable_failure`: assert approved/version remain durable and dispatch fields report `failed`.
  - `helper_timeout_records_unknown_without_claiming_dispatch_failed`: assert dispatch is `unknown` and retry remains version-bound.
  - `dispatch_completion_does_not_overwrite_worker_lifecycle`: change status/version to executing between attempt begin/finish and assert the execution status remains unchanged while the attempt result is recorded.
  - `concurrent_dispatch_attempt_is_rejected_until_first_attempt_is_stale`: assert a second request cannot begin a fresh attempt within 30 seconds, and an older dispatching attempt becomes retryable as unknown.
- [ ] **Step 2: Run the focused tests and confirm failure**
  - Run: `cargo test --locked --manifest-path Cargo.toml deploy_requests`
  - Expected: missing stage/result fields or wrong transition results.
- [ ] **Step 3: Implement typed stage outcomes and redacted diagnostics**
  - Log only request ID, stage, stable error category, and technical error text; never log headers, session tokens, or helper input JSON.
  - Persist `dispatching` with an incremented attempt number after verifying request ID, approved status, and exact version; mark an attempt older than 30 seconds `unknown` before permitting retry.
  - Finish by request ID plus attempt number, never by lifecycle version; do not write lifecycle `status` from dispatch completion, since the worker may already have advanced it.
  - Reuse the pre-existing root authorization result inside the approval action instead of checking Identity twice in the same Platform handler. Keep the independent root check in the root helper. The retry endpoint performs its own same-origin and root checks.
  - Keep `retry_dispatch` limited to the existing exact approved version and update the same dispatch attempt fields.
- [ ] **Step 4: Verify focused lifecycle tests pass**
  - Run: `cargo test --locked --manifest-path Cargo.toml deploy_requests`

### Task 3: Align root-helper Identity endpoint validation with both topologies

**Files:**
- Modify: `src/bin/vapor_platform_ops.rs`
- Test: `src/bin/vapor_platform_ops.rs`

**Interfaces:**
- Add `valid_identity_authz_endpoint(&str) -> bool` for only the two exact configured loopback URL forms: monolith `http://127.0.0.1:7110/api/identity/v1/internal/authorization/check`, and services mode `http://127.0.0.1:7113/v1/internal/authorization/check`.

- [ ] **Step 1: Add failing endpoint tests**
  - `accepts_monolith_identity_authorization_endpoint` and `accepts_services_identity_authorization_endpoint`.
  - `rejects_unexpected_identity_auth_endpoint`: reject wrong host, port, scheme, route, userinfo, query, and fragment.
- [ ] **Step 2: Run the focused helper tests and confirm failure**
  - Run: `cargo test --locked --manifest-path Cargo.toml --bin vapor_platform_ops valid_identity_authz_endpoint`
- [ ] **Step 3: Implement exact two-topology validation**
  - Keep endpoint values sourced from the existing root-owned configuration file; do not accept operation input as endpoint configuration.
- [ ] **Step 4: Verify focused helper tests pass**
  - Run: `cargo test --locked --manifest-path Cargo.toml --bin vapor_platform_ops valid_identity_authz_endpoint`

### Task 4: Render decision and deployment stages in Operations

**Files:**
- Modify: `src/admin.rs`
- Test: `src/admin.rs` or existing admin-route tests, using the project's current test style.

**Interfaces:**
- The request card labels the human decision separately from deployment stage.
- Approval and dispatch errors are read as bounded text and inserted with `textContent`; no response content is interpreted as HTML.
- After any failed action, refresh the row from the API. Offer **Retry dispatch** only when the refreshed request is still approved and `retry_eligible` is true.
- The introductory copy explicitly says that the one-click approval records the decision and attempts to queue the deploy.

- [ ] **Step 1: Add failing rendering/response tests**
  - `deploy_approval_page_labels_decision_and_dispatch_separately`: assert separate accessible labels exist.
  - `deploy_action_feedback_shows_server_message_as_text`: assert stage message is appended with `textContent`, never `innerHTML`.
  - `dispatch_retry_is_rendered_only_for_approved_retryable_requests`: assert an approved request with not-attempted/failed/unknown dispatch exposes retry, while a fresh dispatching, queued, executing, or terminal request does not.
- [ ] **Step 2: Run the focused admin tests and confirm failure**
  - Run: `cargo test --locked --manifest-path Cargo.toml admin`
- [ ] **Step 3: Implement the compact two-stage row and response handling**
  - Preserve the current one-click approval flow and page layout; change only deployment cards and their action feedback.
  - Parse the bounded structured response, display its safe `message`, refresh the list after both success and failure, then derive actions from refreshed state.
- [ ] **Step 4: Verify focused admin tests pass**
  - Run: `cargo test --locked --manifest-path Cargo.toml admin`

### Task 5: Whole-change review and validation

**Files:**
- Review: all files changed by Tasks 1–4.

- [ ] Inspect the final diff for status/version races, migration compatibility, and sensitive-data logging.
- [ ] Run: `cargo test --locked --manifest-path Cargo.toml`
- [ ] Run: `cargo clippy --locked --manifest-path Cargo.toml --all-targets -- -D warnings`
- [ ] Verify the `cargo test --bin vapor_platform_ops` target is included in the full Linux CI build; Windows compilation must remain unaffected by the `cfg(unix)` helper changes.
- [ ] Inspect the untracked workspace items and stage only this feature's files if a commit is requested.
- [ ] Report local validation separately from live VPS deployment; do not claim the production flow works until the deployed server has been exercised.

## Execution review — 2026-11-04

Implemented the migration, durable dispatch attempt state, explicit response stages, helper topology validation, and Operations UI changes. Focused tests cover existing-database migration/idempotence, helper rejection persistence, stale-attempt handling, worker lifecycle races, exact helper URLs, and UI stage/retry rendering. The migration test was first run against the missing function and failed as expected; endpoint/UI tests were added alongside their implementation rather than through a separate red phase.

Validation: full `cargo test --locked --manifest-path Cargo.toml` passes (43 tests). Regular Clippy completes with existing warnings in shared service code and the existing large `Response` error types. Strict `-D warnings` therefore does not pass on the current baseline; rerunning with those existing `dead_code` and `clippy::result_large_err` lints allowed completes successfully. The Linux CI recipe runs `cargo test` for the root package, including the helper binary tests. No live VPS deployment was performed.
