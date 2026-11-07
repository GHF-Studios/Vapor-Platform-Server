# Retire failed deployment dispatches

## Intent

Give a root operator a normal, auditable way to close an approved deployment
request whose dispatch definitively failed before a worker was queued. This
removes the need for direct SQLite edits during guarded recovery.

## Goals

- Allow closure only for the exact current request version in `approved` with
  `dispatch_state = 'failed'`.
- Require the existing same-origin and Identity-backed root authorization.
- Record the operator profile, timestamp, reason, and a clear terminal result.
- Keep the failed request in deployment history and remove it from retryable
  approvals.
- Show the action only on records that satisfy the same server-side condition.

## Non-goals and safety boundary

- Do not retire `unknown`, `dispatching`, `queued`, or `executing` work. Those
  states may represent a live or already-started worker.
- Do not introduce a generic cancellation operation or change deployment
  status vocabulary in this small repair.
- Do not directly stop workers or alter service deployments.

## Approaches

1. Keep using direct SQL for stale approvals. Rejected because it bypasses the
   admin authorization and audit path.
2. Add a narrowly guarded Platform API/UI operation for failed dispatches.
   Chosen because Platform owns these deployment records and this reuses its
   existing root-operator authorization.
3. Add a broad cancellation state for every deployment lifecycle stage.
   Deferred because it would require a wider status migration and risks
   misrepresenting uncertain worker outcomes.

## Behavior

The Deployments page offers “Close failed dispatch” only when the request is
approved, the dispatch state is failed, and its target selection is valid. The
operator supplies a short reason and explicitly confirms. The server performs
one conditional update using ID, expected version, status, and dispatch state;
on success it changes the request status to failed, increments the version,
records the root operator and reason, and records that no worker was queued.
If the request changed or no longer qualifies, the action returns conflict and
does not modify it.

## Verification

Tests cover the allowed transition, stale version, wrong status, uncertain
dispatch state, missing/invalid reason, and UI visibility/endpoint wiring.
