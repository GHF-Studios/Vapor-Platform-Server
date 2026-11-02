# Human-Approved Operations Design

## Intent

Let trusted automation prepare deploy and permission-change requests, while a
human explicitly reviews and approves each consequential action in Vapor's web
dashboard. The dashboard and APIs should provide an auditable, usable path for
Leslie and later approved automation without making a request credential an
approval credential.

## Current state

- Platform deployment automation follows `main`: pushes can trigger the
  workflow, and the VPS timer periodically invokes the deployment service.
- The deployer resets its checkout to the current branch head, so the existing
  operation record does not identify an approved immutable deployment request.
- Platform Operations is read-only and reports only the latest deploy/restore
  status.
- Identity role grant/revoke browser forms and API handlers take effect
  immediately for a root-authorized operator.
- Platform owns deploy orchestration; Identity owns roles, permission policy,
  and the audit history. Their queues and execution rules must remain in those
  services.

## Goals

- Every normal production deploy and dashboard role change has an explicit
  pending request and a separate human approval action.
- A deploy approval applies only to the exact Platform revision and pinned
  submodule revisions shown during review.
- A permission approval applies only to the exact role, action, and verified
  Steam/GitHub-linked subject shown during review.
- A narrow API supports direct request submission by automation and humans.
  Request-only credentials cannot approve, reject, execute, alter a request, or
  grant permissions directly.
- Approval and execution outcomes are auditable, durable, and visible from the
  shared admin toolbench.
- Existing bootstrap/emergency access remains available only for first-root
  recovery and documented server-local break-glass operations.
- The deployment remains one VPS / one Platform process with service-owned
  state, APIs, and rules.

## Non-goals

- General-purpose remote shell, arbitrary command, SQL, or file-write APIs.
- A generic cross-service ticket database or moving Identity logic into
  Platform.
- Automatic approvals, self-issued approval credentials, or approval via a
  deploy request token.
- Build Cache VM workers, Vapor Client cache restoration, artifact publication,
  or automatic application of community content. These remain separate work.
- Live VPS deployment as part of implementation.

## Approaches considered

1. **GitHub Environment approval only.** Small and mature for workflow gates,
   but does not provide the requested Vapor dashboard/API or one place for
   permission-change requests.
2. **Central Platform action queue.** One simple dashboard/API, but it would
   make Platform own Identity permission business logic and its audit data,
   violating repository ownership rules.
3. **Service-owned approval queues behind the shared dashboard (selected).**
   Platform owns deploy requests/execution; Identity owns permission requests,
   validation, and role mutation. Each service exposes its own API and admin
   section. The Platform shell links to both. This aligns with the agreed
   architecture and preserves independent service rebuild/deploy.

## Design

### Actors and credentials

- **Human operator:** signs in through Vapor Identity. A current root dashboard
  session may create, review, approve, reject, or cancel requests. Browser
  mutations require same-origin/CSRF protection and explicit confirmation.
- **Automation requester:** uses a revocable, expiring, request-only API
  credential issued by a root operator. Its scope identifies permitted request
  kinds. Identity owns the credential and its audit/revocation. The credential
  can create requests and inspect only the requests it created; it cannot
  approve/reject, read private service state, execute work, or call immediate
  role mutation routes.
- **Worker:** a narrowly scoped host-side worker consumes already-approved
  Platform deploy requests. It does not accept arbitrary commands or revisions
  directly from the network. Identity applies an approved role change inside
  its own service after revalidating the subject and invariants.

The API is the canonical interface; dashboard actions call the same service
handlers. No browser-only parallel authorization path is introduced.

### Request envelope and states

Both services use the same documented conceptual envelope, but own their
storage and transition rules:

`pending -> approved -> executing -> succeeded | failed`

`pending -> rejected | cancelled | expired`

Terminal requests cannot be changed or replayed. Approval binds to a hash of
the immutable request body. A queue action uses an expected-state/version
check, so duplicate approval or worker delivery cannot execute twice. Requests
expire after a configured finite lifetime. Each transition records actor,
timestamp, reason where applicable, and outcome; secrets and internal database
IDs are not exposed in public responses.

### Platform deploy requests

- The request records an allowlisted action (`deploy`), exact 40-character
  commit, source repository identity, expected root tree hash, and the pinned
  submodule gitlinks. It may include a concise CI summary and links, not a
  caller-controlled command or filesystem path.
- Only revisions reachable from the configured protected deployment branch
  are eligible. The worker re-fetches and verifies the exact commit and
  gitlinks; it never substitutes a newer branch head after approval.
- The dashboard shows revision, commit/tree identity, submodule pins, CI
  result, request actor, age/expiry, and a clear diff/runbook link before
  approval.
- CI may automatically create a pending request after required checks succeed.
  It cannot approve or start a deploy. Direct API callers can create the same
  request using a request-only credential.
- The approval handler records approval only. A dedicated systemd worker polls
  or is signaled to claim approved tickets with an atomic claim. The worker
  runs the existing allowlisted deploy operation for the approved revision,
  reports progress/result, and writes the existing operations history.
- Remove the current “every push/timer means deploy latest main” behavior.
  CI still validates updates; production changes happen only for an approved
  request. A manually invoked documented break-glass path remains separate and
  is audited.

### Identity permission requests

- Replace normal immediate root-dashboard/API grant/revoke actions with
  requests. Existing validation of roles, linked Steam/GitHub identities,
  `root` implication, and last-root protection is retained and repeated when an
  approved request is applied.
- The request captures the exact grant/revoke, role, and subject resolved from
  linked external identities. It does not rely on a caller-supplied internal
  profile ID.
- Only a current root human session can approve/reject. A request-only API
  credential can submit a request but cannot call the existing immediate
  mutation functions.
- Approval applies the change transactionally with an audit event that names
  both requester and approver. If state changed since request creation, the
  approval becomes stale and must be re-requested rather than applying a
  different result.
- First-root bootstrap and last-root recovery remain explicit break-glass
  operations with their present safeguards; they are not silently converted
  into automation-accessible paths.

### Dashboard and routing

- Add an Operations approval area for Platform deploy requests and an
  Identity-owned permission-request section. Both use the existing shared
  admin shell and existing Identity root-session authorization.
- Each service owns its page, API routes, state, validation, and audit events.
  The shared shell provides navigation and may show aggregate pending counts
  only through authenticated service-owned summary endpoints.
- Public proxy routes expose versioned request APIs. Worker-only claim/report
  routes stay loopback-only and are explicitly denied by Caddy at public paths.
- Every mutation uses `Cache-Control: no-store`, bounded request bodies,
  strict DTOs that reject unknown fields, CSRF/origin checks for cookies,
  idempotency/version checks, rate limits, and fail-closed authorization.

### Persistence and recovery

- Platform stores deploy requests and transition audit in Platform-owned
  SQLite state under `/var/lib/vapor-server/platform`.
- Identity stores permission requests in its existing SQLite database.
- Whole-system export/import includes both databases with service quiescing and
  integrity validation. Restore preserves pending/terminal states exactly;
  expired pending tickets cannot be executed.
- No credentials, signing keys, cookies, or raw API tokens are exported in
  clear text. Request-only credentials are stored hashed and are revocable.

## Phased implementation

1. **Contracts and credential scopes:** define shared conceptual states and
   versioned service-specific APIs; add Identity-issued request-only credentials
   with expiry, revocation, and request-kind scopes.
2. **Platform deploy queue:** persist requests, render review/approval UI,
   update CI to request only, add exact-revision worker claim/execute/report,
   and remove unapproved push/timer deployment behavior.
3. **Identity permission queue:** migrate dashboard grant/revoke forms and
   normal API operations to request/approve/reject; preserve invariants,
   external identity binding, and audit attribution.
4. **Shared admin presentation and operations:** link queues in the toolbench,
   show pending counts/status, include migration/export/restore behavior and
   operator recovery docs.
5. **Verification and handoff:** exercise unauthenticated, request-token,
   human approval, stale revision, duplicate delivery, expiration, worker
   failure/retry, last-root, and export/restore cases. Prepare deployment
   instructions for Leslie's manual authorization; do not deploy automatically.

## Acceptance criteria

- A main-branch update can pass CI and create a pending deploy request but
  cannot deploy before root approval.
- Approval displays and binds to the exact root commit and every submodule
  gitlink; altering any ref or ticket after approval does not affect execution.
- The approved deploy can be claimed at most once, and result/failure is visible
  in Operations history.
- A request-only API key can submit permitted requests but every attempt to
  approve, reject, mutate roles, claim work, or execute a command is denied.
- Root session approval/rejection is protected against CSRF and recorded with
  actor and time.
- Permission changes have distinct request and approval steps; approval
  revalidates all existing Identity invariants and stale requests cannot apply.
- No normal deployment or permission mutation path bypasses approval. Bootstrap
  and break-glass paths are explicit, documented, server-local, and audited.
- `cargo test`, shell/Python checks, and service contract tests pass in CI; no
  test invokes the production worker or modifies live VPS state.
- No production deploy occurs until Leslie manually authorizes it after review.

## Calls made for implementation

- The first automation caller model is an Identity-issued request-only API
  credential, not an anonymous endpoint, shared root cookie, or SSH command.
- The deploy request is a source revision deployment, not a binary artifact
  upload. Build-artifact publication remains owned by the Artifact service and
  requires its own approval contract later.
- Approval is deliberate human confirmation, not mandatory two-person
  separation-of-duties; the root operator may approve a request they created.
- Standard pending requests are finite-lived and immutable; a change requires
  cancellation and a new request.
