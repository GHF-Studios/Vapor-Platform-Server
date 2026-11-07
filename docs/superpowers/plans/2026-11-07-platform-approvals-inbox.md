# Platform Approvals Inbox Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a protected shared Approvals inbox for pending Platform deployment, Platform SSH access, and Identity permission requests.

**Architecture:** Platform renders the inbox and fetches each owner’s existing list API. Browser decisions are posted directly to the corresponding owner API using the same-origin session; no request state or audit is copied into Platform. Deploy execution, active SSH grants, identity management, and history remain in their existing work areas.

**Tech Stack:** Rust, Axum, embedded HTML/JavaScript, existing Platform and Identity APIs.

**Spec:** `docs/superpowers/specs/2026-11-06-vapor-platform-operations-and-recovery-redesign.md` (Admin workbench and Request and operation lifecycle).

## Global Constraints

- Platform is a shared interface, not a second authority or record store.
- Decision writes go directly to the service that owns each request.
- A source outage is shown locally and must not hide other sources.
- Keep requests with missing/invalid deployment targets unapprovable.
- Preserve owner API version checks, explicit confirmations, authorization, and audit behavior.

## Review Focus

- A failed or malformed source response leaves other source sections usable.
- A stale version/conflict refreshes the source and does not claim success.
- A deploy decision saved before dispatch failure is reported as saved with dispatch unresolved.
- Identity rejection requires a non-empty bounded reason.
- Only pending requests appear in the inbox; decided requests remain in owner history.

---

### Task 1: Add the protected Approvals view

**Files:**
- Modify: `src/admin.rs`
- Test: `src/admin.rs` unit tests

**Interfaces:**
- `OperationsView::Approvals` maps to `view=approvals` and the protected `/admin/operations?view=approvals` page.
- The page exposes independent containers for deployment, SSH, and Identity pending requests.

- [x] Add a failing test that `OperationsView::parse(Some("approvals"))` selects the new view and its rendered page names the three owner sources.
- [x] Run the targeted test and confirm it fails because the view is absent.
- [x] Add the view to navigation, parse/name mappings, landing-page link, and render three independent inbox sections.
- [x] Run the targeted test and confirm it passes.

### Task 2: Add owner-routed pending review actions

**Files:**
- Modify: `src/admin.rs`
- Test: `src/admin.rs` UI contract tests

**Interfaces:**
- GET endpoints: `/api/admin/deploy-requests`, `/api/admin/ssh-access/requests`, `/api/identity/v1/admin/permission-requests`.
- Decision endpoints: existing approve/reject routes under each same owner API.
- The inbox contains pending items only and shows source-specific unavailable/authentication states.

- [x] Add contract tests for the three source URLs, their separate approve/reject destinations, and the per-source partial-error behavior.
- [x] Render pending-only cards with exact subject/target and explicit confirmations; retain SSH grant scope/duration selection and Identity rejection reason.
- [x] Route decisions to owner endpoints, refresh the affected source, and show persisted-decision versus follow-up dispatch status distinctly.
- [x] Run the Platform test suite and `git diff --check`; validate the embedded JavaScript with `node --check`.

### Task 3: Verify navigation and boundary consistency

**Files:**
- Modify: `src/admin.rs`
- Modify: `docs/server-domain-boundaries.md` only if current route map omits Approvals.
- Modify: `README.md` only if its admin navigation summary omits Approvals.

- [x] Confirm Deployment and SSH management pages continue to show execution/grant lifecycle, while the inbox shows only unanswered requests.
- [x] Run `git diff --check` and inspect the final diff for service ownership leaks.
