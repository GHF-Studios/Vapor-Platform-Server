# Root-Operated Deployment Approval Design

## Intent

Let a trusted operator approve and queue one exact Platform deployment over an
already authorized root SSH session. This removes the browser click as a
required step for the owner-authorized automation workflow while preserving an
explicit approval record for each exact request.

## Decisions and constraints

- The approval entrypoint is a typed root-only command in the existing deploy
  queue tool. It is not an HTTP route and is not available to the `vapor`
  service account.
- The operator supplies the request UUID, expected full commit SHA, and a
  short reason. The command verifies the immutable request body, official
  repository, `main` branch, exact commit, supported target, pending status,
  and unexpired approval window before changing state.
- The command records a null Identity profile plus a decision reason stating
  that approval came from the local root operator. It must not impersonate a
  browser user.
- Approval and request-scoped worker dispatch are one typed operation. A
  failed `systemctl start` leaves the approval recorded and dispatch retryable.
- No auto-approval policy is added. Root SSH remains the delegated trust
  boundary; browser-based root access remains a separate, disabled feature.
- The installer accepts only the exact current legacy sudoers policy or the
  exact one-line policy verified on the live VPS. It keeps either until the
  replacement service passes health checks, then retires it.

## Approaches

1. **Root-only typed command (selected):** narrow, auditable, and uses the
   existing root SSH trust boundary; no new internet-facing authority surface.
2. **Auto-approve every main-branch CI request:** lower friction but removes
   the per-request decision and grants broader standing authority than asked.
3. **New Identity machine approver:** clean for a future multi-operator
   product, but adds credential lifecycle and Identity work unnecessary for
   the current owner-operated pre-alpha.

## Out of scope

- Arbitrary shell access or a browser terminal.
- Changing existing Identity/browser approval semantics.
- Automatically approving all commits or other repositories.
- Retiring the fixed deploy worker or its exact commit/source validation.
