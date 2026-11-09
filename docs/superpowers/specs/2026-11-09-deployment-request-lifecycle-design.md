# Deployment request lifecycle

## Problem

Deployment requests are immutable records, but the overview mixed approval
counts with execution follow-up and the deployment workbench grouped uncertain
dispatch state with queued work. Old pending requests for earlier `main`
commits could also block the current request in the root worker even though
the API would refuse to approve the stale revision.

## Design

- Keep requests as append-only lifecycle records. Do not persist “superseded”
  markers based on an external `main` lookup: request creation can race a newer
  GitHub `main` revision and incorrectly invalidate the newer request.
- Treat GitHub `main` as the authority for currentness. Approval and dispatch
  require exact current-main equality; the UI labels a pending row stale only
  when it has a verified current-main value and the commits differ.
- An older pending row blocks an approval/worker candidate only when it has the
  same commit. A pending row for a different commit is stale relative to the
  current candidate and cannot block it. Earlier approved queued/dispatching/
  unknown and executing work still blocks newer execution because it may have
  reached the host.
- The root worker's `claim`, `approve-and-queue`, and `next` paths follow the
  same ordering rule as the HTTP approval path.
- Show queued/running, uncertain outcome, and dispatch failure in separate
  deployment workbench groups. Only verified-current approvals count as
  awaiting approval; execution follow-up is reported separately.
- Keep stale pending rows visible in the approvals page's collapsed section
  and in the request records; do not claim they are in the operation-history
  feed.

## Safety boundaries

- A request is created only after GitHub OIDC verification and a current-main
  check.
- No row that is queued, dispatching, unknown, or executing is skipped by the
  ordering predicates.
- Exact-current-main checks remain mandatory at approval, dispatch retry, and
  worker deployment checkout.
- The change does not delete requests, run stale snapshots, or write stale
  classification based on a non-atomic external observation.

## Acceptance

- Different-commit older pending rows do not block the current request in API
  approval or any root-worker queue operation.
- Same-commit pending rows and earlier potentially-running work still block.
- Unverified `main` state does not inflate the “awaiting approval” count.
- Queued/running, uncertain outcome, and failed dispatch are labeled
  separately; stale retry remains disabled by current-main equality checks.
- Tests cover both Rust and Python ordering paths and each UI category.
