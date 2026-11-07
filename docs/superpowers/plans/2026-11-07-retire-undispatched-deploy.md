# Plan: retire a failed deployment dispatch

1. [x] Add a regression test for the conditional terminal transition; verify it
   failed before implementation and then passed after the implementation.
2. [x] Implement the same-origin, Identity-root-authorized endpoint with a
   bounded operator reason and optimistic version/state checks.
3. [x] Add a dashboard action only for approved requests with failed dispatch,
   including explicit confirmation, reason capture, and outcome feedback.
4. [x] Run the focused Rust and admin frontend tests, the complete repository
   test recipe, JavaScript syntax validation, and `git diff --check`.
5. [x] Review the diff against the design and deployment/recovery invariants.

## Verification result

The complete `deploy/scripts/test.sh` passed on Windows via Git Bash: Atlas
validation/build, 90 deployment-helper Python tests (10 skipped), root Platform
tests, and all service submodule tests passed. The embedded admin JavaScript
parsed successfully with Node, and `git diff --check` passed. Existing Rust
dead-code warnings from the diagnostics, docs, and identity crates remain.
