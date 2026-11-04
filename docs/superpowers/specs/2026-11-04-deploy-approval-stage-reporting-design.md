# Deploy Approval Stage Reporting Design

## Intent

Make the Platform deployment approval action truthful and diagnosable without
changing its one-click operator flow. Today the Approve button both records the
root operator's decision and dispatches the approved request. The UI treats
those as one opaque success/failure, so a dispatch failure can look like an
approval failure and a pre-save failure gives no useful cause.

## Goals

- Keep one explicit **Approve deploy** action.
- Distinguish failure to authorize or persist the decision from failure to
  queue an already-persisted approval.
- Show the decision and execution/dispatch stage on the request row, including
  after a reload.
- Preserve root-session authorization, same-origin checks, exact-revision
  binding, expected-version checks, and the fixed privileged helper boundary.
- Give each failure a safe, bounded, useful explanation in the API response
  and dashboard; write technical causes to the server journal without tokens,
  cookies, or request secrets.

## Non-goals

- Do not bypass approval, provide a manual shell/deploy route, or broaden the
  privileged helper.
- Do not change who may submit, view, approve, or execute requests.
- Do not redesign unrelated Operations or Identity screens.
- Do not split approval and dispatch into separate user actions.

## Approaches considered

1. **Keep the current one-click action and generic error.** No extra work, but
   it preserves the misleading state and leaves production failures opaque.
2. **Separate approval and queueing into two buttons.** This gives each action
   a separate response, but changes the operator flow and is not the chosen
   behavior.
3. **Keep one click and expose durable stage outcomes (selected).** The server
   reports whether the decision was saved and whether dispatch was queued; the
   dashboard renders both stages and retains dispatch failure information for
   reload/retry. This matches the current authority boundary while making its
   actual outcomes visible.

## Behavior

The approval endpoint remains a single root-authorized operation:

1. Validate same-origin request, explicit confirmation, and expected version.
2. Persist `pending -> approved` atomically.
3. Dispatch the exact approved request through the existing fixed helper.
4. Report both stage results. A dispatch failure must never be described as an
   unsaved approval.

The request's decision status remains `approved` if queueing fails, preserving
the existing retry path. The API/list representation gains an explicit
dispatch stage (`not_attempted`, `dispatching`, `queued`, `failed`, or
`unknown`) and a bounded operator-safe dispatch detail. A dispatch attempt
that times out or is interrupted is `unknown`, not a claimed failure; retries
remain safe because the systemd unit is request-specific and the worker claim
is atomic. A repeated request may retry dispatch only while the same approved
request/version remains eligible. The worker's existing atomic claim
continues to own `approved -> executing` and later terminal transitions.

Failure responses identify the stage with stable machine-readable codes:

- Before the database transition: `decision_not_saved` plus a safe category
  such as `root_authorization_unavailable`, `origin_rejected`,
  `stale_or_expired`, or `decision_store_unavailable`.
- After the transition: `decision_saved_dispatch_failed`, with the request ID,
  approved version, and retry eligibility.
- Full success: a structured result containing the persisted decision and
  dispatch state.

The server journal records internal error details at each boundary for root
authorization, decision persistence, helper invocation, and dispatch-state
recording. It must never log session cookies, service tokens, public/private
SSH key material, or full request headers.

The Platform handler should perform its Identity root check once per browser
action. The root-owned helper still independently rechecks the supplied
session, preserving the privilege boundary. The handler must not make a second
redundant Identity call before invoking the helper. The helper's endpoint
allowlist must match the two endpoint forms emitted by
`deploy/scripts/configure-authorization.sh`: the composed monolith route on
port 7110 and the standalone Identity route on port 7113. It must continue to
reject non-loopback hosts, unexpected ports, schemes, paths, credentials, and
query strings.

The UI keeps the current compact request card and one-click approval. On
failure it renders the response body (bounded and text-only) instead of only a
status number, refreshes the request state, and offers retry only when the
server reports a persisted approval eligible for retry. After reload, the row
shows separate **Decision** and **Deployment** states and its last safe stage
detail.

## Data and migration

Extend the Platform-owned deploy-request record using an explicit idempotent
SQLite migration rather than relying on `CREATE TABLE IF NOT EXISTS` to alter
existing state. Persist dispatch stage, attempt timestamp/count, and a
bounded safe failure detail. Existing requests receive the neutral
`not_attempted` dispatch stage. Keep execution `result_detail` separate from
dispatch failure detail.

## Security and ownership invariants

- Platform owns deploy request state and deployment orchestration.
- Identity remains the source of root authorization.
- The web process remains unprivileged; only the fixed root-owned helper may
  queue the per-request systemd unit.
- A retry cannot alter the approved commit, tree, or pinned submodules.
- Both supported Platform topologies use the exact loopback Identity endpoint
  selected by `configure-authorization.sh`; neither topology can select an
  arbitrary helper authorization endpoint.
- Diagnostics are stage-specific and redacted; internal SQL/helper errors are
  journal-only.

## Verification

Cover approval-save failure, successful save plus dispatch failure, complete
success, stale/expired version, dispatch retry, and worker claim racing with
the dispatch response. Verify the dashboard renders each response/state
without injecting server text as HTML. Confirm existing root authorization,
same-origin, and exact-revision tests still pass.
