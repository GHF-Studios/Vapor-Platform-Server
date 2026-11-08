# Root-Operated Deployment Approval CLI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Let an already authorized root SSH operator approve and queue one exact pending Platform deployment request with an auditable reason.

**Architecture:** Extend the existing Platform deploy queue helper with a root-only `approve-and-queue` command. Validate the stored immutable request before an atomic approval transition, then start only its fixed request-scoped systemd worker and record dispatch failure as retryable. Update the host installer to recognize both exact known legacy sudoers policies while preserving post-health retirement ordering.

**Tech Stack:** Python 3, SQLite transactions, systemd, Bash, existing unit and deployment test suites.

**Spec:** `docs/superpowers/specs/2026-11-08-root-deploy-approval-cli-design.md`

## Global Constraints

- The command requires effective UID 0 and accepts no arbitrary command/path input.
- Approval is limited to an unexpired pending request from `GHF-Studios/Vapor-Platform-Server` on `main`, with the exact requested commit and supported deployment target.
- Approval writes `decided_by_profile_id = NULL` and records a local-root decision reason; it never impersonates an Identity profile.
- A failed worker dispatch leaves the decision recorded and the dispatch state retryable.
- The legacy sudoers file is removed only after the new Platform health check.

## Review Focus

- Forged or mutated stored request fields: verify `body_hash` and denormalized columns before approval.
- Wrong repository, branch, commit, expired request, or unsupported target: reject without mutation.
- Concurrent approval/replay: SQLite compare-and-update permits only one transition from pending.
- Worker start failure: preserve approval, mark dispatch failed, and return a useful typed error.
- Sudo policy mismatch: accept only exact reviewed historical forms and retain every other policy.

---

### Task 1: Root-only approval and dispatch command

**Files:**
- Modify: `deploy/scripts/deploy-request-queue.py`
- Test: `deploy/tests/test_deploy_request_queue.py`

**Interface:** `approve-and-queue --id UUID --commit SHA --reason TEXT`.

- [ ] Add tests for valid approval, wrong commit, wrong repository/branch, expired request, mutated request body, replay/concurrent transition, and worker-start failure.
- [ ] Verify each new test fails for the expected missing behavior.
- [ ] Implement strict validation, one atomic pending-to-approved transition, and fixed `systemctl start --no-block vapor-deploy@<uuid>.service` dispatch.
- [ ] Require root in the command entrypoint; keep the pure transaction behavior testable without root.
- [ ] Run the focused Python suite.

### Task 2: Accept the exact live legacy sudo policy

**Files:**
- Modify: `deploy/scripts/install-platform-ops.sh`
- Modify: `deploy/scripts/retire-platform-ops-sudoers.sh`
- Test: `deploy/tests/test_platform_ops_installer.py`

- [x] Add a regression assertion for the exact one-line policy observed on the VPS.
- [x] Make install and retirement recognize the exact one-line and exact four-line historical forms, rejecting all others.
- [ ] Run the focused installer test and Bash syntax checks.

### Task 3: Document, verify, and publish

**Files:**
- Modify: `deploy/README.md`

- [ ] Document the root-only command, the trust boundary, and its audit behavior.
- [ ] Run `deploy/scripts/test.sh`, `git diff --check`, and branch CI on Linux and Windows.
- [ ] Review the final diff and push the tested source. Use the root-only command to approve the resulting exact main-branch deployment request.
