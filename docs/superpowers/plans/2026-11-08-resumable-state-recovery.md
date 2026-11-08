# Resumable state-only recovery implementation plan

**Design:** `docs/superpowers/specs/2026-11-08-resumable-state-recovery-design.md`

## Global constraints

- Preserve the state-only archive boundary: no `/etc`, secrets, source checkout,
  systemd configuration, or recovery code enters the archive.
- A retry is bound to the exact archive digest and Platform commit.
- No non-empty or markerless host is treated as a resumable recovery.
- Do not delete or overwrite unexpected host data during recovery or cleanup.
- Final completion requires configured provider readiness and explicit owner
  confirmation after browser sign-in checks.

## Task 1: Define and test the recovery marker contract

**Files:**
- Add: `deploy/scripts/state_only_recovery_marker.py`
- Add: `deploy/tests/test_state_only_recovery_marker.py`
- Modify: `deploy/scripts/test.sh`

- [ ] Add tests for marker creation, strict parsing, allowed phases, digest and
  revision binding, atomic replacement, and completion cleanup.
- [ ] Run tests and observe expected failures before implementation.
- [ ] Implement a small marker helper with no shell execution and no secret
  fields; reject symlinks, unsafe ownership/modes, duplicate/unknown fields,
  and unexpected directory contents.
- [ ] Re-run focused tests and Python syntax validation.

## Task 2: Add the resumable state-only host lifecycle

**Files:**
- Modify: `deploy/scripts/recover-state-only-host.sh`
- Modify: `deploy/scripts/README-state-only-recovery.txt`
- Modify: `deploy/tests/test_state_only_fresh_host_contract.py`
- Add: `deploy/tests/test_state_only_fresh_host_resume_contract.py`

- [x] Add `--resume` and `--confirm-provider-checks` argument handling.
- [x] Create the marker only after archive/source/host preflight and validated
  operator inputs; update phases atomically.
- [x] On resume, require the exact marker/archive pair and verify any existing
  config/source checkout before reusing it.
- [x] Preserve interrupted checkout/config for retry; stop partial services on
  failure and retain the marker. Refuse unexpected paths.
- [x] Make provider readiness and owner browser-check confirmation distinct
  states; clear the marker only after both are satisfied.
- [x] Document first run, interrupted retry, final confirmation, and what to do
  when a marker or path fails validation.
- [ ] Add contract tests that prove archive validation still precedes package
  mutation and that resume paths enforce the pinned inputs.

## Task 3: Review and verify

- [ ] Run focused Python tests, shell syntax, and `git diff --check`.
- [ ] Run the complete Platform test recipe.
- [ ] Review marker lifecycle, failure cleanup, provider check flow, and archive
  boundaries against the design.
- [ ] Commit/push only after fresh verification; do not deploy without the
  normal approval flow.
- [ ] Record that a disposable Ubuntu restore drill remains required before
  claiming end-to-end recovery.
