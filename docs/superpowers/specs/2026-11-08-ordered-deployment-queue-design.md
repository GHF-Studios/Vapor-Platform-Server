# Ordered deployment queue

## Goal

Make a wrong-order Platform deployment impossible even when several CI requests
are approved close together, workers wait behind the shared deploy lock, or an
old failed dispatch is retried later.

## Rules

- The queue has one stable total order: request creation time, then request ID
  as a deterministic tie-breaker.
- The oldest open request is the only deployment request that can be approved.
  Older pending requests must be rejected, approved, or expire before a newer
  request can be approved.
- Workers recheck queue order after acquiring the global deployment lock. UI
  ordering and disabled buttons are presentation only; the API and worker are
  authoritative.
- A request in `queued` remains queued while it waits for the lock. The short
  dispatch timeout applies only while dispatch is still being recorded, not to
  a worker that the system manager already accepted.
- A completed worker wakes the next approved queued request. It never approves
  a pending request automatically.
- Before changing live code, a worker fetches protected `main` and requires the
  approved revision to equal its current head. A retry of a superseded ancestor
  is closed without deployment; deploying the newer head already includes its
  ancestors.
- CI checks that its tested SHA is still `main` before submission, and the
  Platform API repeats that check when accepting a request. The approvals API
  exposes the current `main` SHA; it refuses stale approvals before recording
  them, and the inbox disables their approval action while leaving rejection
  available. The worker check remains the final race-safe guard if `main`
  advances after approval.
- Known pre-claim dispatch failures do not block the queue. An unknown outcome
  does block it until an operator resolves it, because a worker may still have
  changed the host.

## User experience

The approvals inbox puts older deployment requests first and shows which one is
next. Later approvals are visibly unavailable until the earlier request is
decided. Server-side conflicts explain that ordering is the reason. The
Deployments view continues to show dispatch, worker, and final result as
separate stages.

## Verification

Required automated coverage includes stale queued work, older-request gating
at approval and claim, ordered wake-up after a worker completes, stale dispatch
recovery, and the worker's current-main guard. The new lookup parser and exact
SHA comparison have unit coverage; route-level tests that assert stale request
creation and approval leave the database unchanged remain required before this
change is fully verified. The host is checked after each sequential live
deployment.
