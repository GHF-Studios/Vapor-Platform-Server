# Pin deployment topology in approval requests

**Goal:** Make each human-approved deployment identify whether it will install
the `services` or `monolith` topology, then have the worker execute only that
approved topology.

**Design reference:** `docs/superpowers/specs/2026-11-06-vapor-platform-operations-and-recovery-redesign.md`,
section “Deployment target is part of the approval”. The owner-approved target
architecture is independent services plus the shared Platform gateway. The
current live host remains monolith until a services-target request is reviewed
and approved.

## Contract

- Add required `server_mode` (`services` or `monolith`) to the immutable request
  JSON and include it in the body digest.
- GitHub Actions submits `services` as the target for new main-branch requests.
- The dashboard displays the target. A legacy request with no target remains
  readable/rejectable but cannot be approved or dispatched.
- The request worker validates, extracts, and passes the approved mode to
  `deploy.sh`; host configuration is updated by deployment only after approval.
- The deploy helper verifies request ID, version, commit, and mode together.
- A previously approved legacy request fails closed with a clear terminal
  dispatch result, rather than leaving a permanently approved/dispatching row.

## Tasks

1. **Define and validate the request field.** Add `server_mode` to the Rust
   request and summary DTOs; validate the two exact accepted values; expose
   `null` for requests whose stored JSON has no mode. Prove valid modes pass and
   an unknown value fails.
2. **Render the approved target.** Add a concise “Target: Services” or
   “Target: Monolith” line to each card. For a legacy ticket, show that its
   target is missing and disable only the Approve action; keep Reject available.
3. **Submit the topology from CI.** Add the explicit `services` target to the
   GitHub request payload and change the summary to say what CI actually
   verified, without implying the deployment has already happened.
4. **Bind execution to the request.** Update the queue helper to validate and
   return `server_mode`; fail legacy approved requests into a visible terminal
   failure; extend `verify` to require the same mode; pass it through the worker
   and deployment script. `deploy.sh` must not fall back to host mode for an
   approved deployment. If the fixed helper returns a bounded structured
   failure, log its safe detail so the operator can distinguish Identity,
   request-state, and systemd queue failures.
5. **Document operator semantics.** Update `docs/github-deployment.md` and
   recovery/operation records to explain that the requested target is shown and
   pinned before approval, and that current host mode changes only when that
   request executes.

## Verification

- Rust unit tests: accepted/rejected target values and legacy request summary.
- Python queue tests: valid request includes mode; missing/invalid mode is
  terminally failed without deployment; verify rejects mode mismatch.
- Workflow contract test: main request payload explicitly sets `services`.
- Shell tests/static checks: the approved worker passes the claimed mode and
  deployment verification requires it; bootstrap remains explicit and
  independent of the approval worker.
- Run the Platform test suite, formatting, and deployment script checks. Then
  inspect the diff and confirm no untracked VPS scratch files were staged.

## Self-review

- Existing `monolith` remains representable for explicit recovery or migration
  requests; ordinary CI requests target the approved independent-service
  architecture.
- Existing requests are not rewritten because their body hashes are immutable.
  Missing target is surfaced as legacy and cannot be approved.
- This change does not alter the currently deployed VPS or approve, reject, or
  dispatch the already-pending request `5338a3de-fd38-4c89-94ed-5afba23722ab`.
- The current pending ticket may be superseded by a new request after this
  change reaches main; that is an explicit approval transition, not an implicit
  deployment.
