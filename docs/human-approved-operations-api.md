# Human-approved deployment operations

## Flow

1. A push to `main` runs the composed CI workflow, including every root-level
   service submodule at the gitlink pinned by that root commit.
2. After CI succeeds, GitHub Actions submits the full root commit, root tree,
   every submodule commit, and its workflow URL using a request-only Identity
   credential. CI has no VPS SSH access and cannot approve or execute deploys.
3. A root operator opens `/admin/operations`, reviews the exact commit/tree and
   expandable submodule pins, and explicitly approves or rejects the ticket.
4. Root approval immediately requests a request-specific systemd unit. The
   worker rechecks
   the request digest, branch reachability, root tree, and all gitlinks before
   checking out the exact approved revision; it also rejects changed or
   non-first-party submodule URLs before fetching them. It invokes only the
   fixed deploy script and never accepts shell commands, paths, refs, or
   environment values from the request.
5. Each ticket is atomically claimed once. If dispatch fails before the worker
   claims it, the root dashboard offers a retry. A failed or rejected execution
   needs a new CI request and approval. If the worker stops after claiming the
   ticket, the root dashboard can reconcile it only after the fixed helper
   confirms that request's systemd unit is inactive or failed. Reconciliation
   records a terminal failure and never retries the deployment.

The first installation is an explicit operator bootstrap:

```bash
sudo deploy/scripts/bootstrap-ubuntu.sh
sudo deploy/scripts/deploy.sh --bootstrap
```

Bootstrap is refused when `/opt/vapor-platform-server` already contains a
checkout or other files. It is not used for updates.

## Enable CI ticket creation

1. In the Identity dashboard at `/admin/identity`, create an automation
   credential with only **Create Platform deploy requests** selected. Copy the
   one-time secret when Identity shows it.
2. Add that value to the Platform repository's GitHub Actions secrets as
   `VAPOR_DEPLOY_REQUEST_TOKEN`.
3. Add the public base URL (for example `https://vapor.ghf-studios.site`) as the
   repository variable `VAPOR_DEPLOY_PUBLIC_BASE`, without a trailing slash.
4. Keep the server-local Identity-to-Platform verification token in the
   protected `/etc/vapor-server/identity.env` and `platform.env` files. The
   bootstrap/authorization script provisions matching, distinct service
   credentials; they are not the GitHub request credential.

If either GitHub setting is absent, composed CI still runs and reports that it
skipped ticket creation. It does not fall back to SSH or a direct deploy.

## Platform API

All responses from the request API are `Cache-Control: no-store`.

| Route | Authority | Purpose |
| --- | --- | --- |
| `POST /api/admin/deploy-requests` | Identity credential scoped to `platform:deploy-request:create` | Create or idempotently return the active ticket for a commit |
| `GET /api/admin/deploy-requests?limit=50&cursor=…` | Root browser session, or a request credential | Returns `{ items, next_cursor }`; root sees the queue, while a request credential sees only tickets created by that credential. Page size is 1–200; omit `cursor` for the first page and follow the returned cursor for older records. |
| `POST /api/admin/deploy-requests/{id}/approve` | Root browser session, same-origin, explicit confirmation, expected version | Approve one pending ticket |
| `POST /api/admin/deploy-requests/{id}/reject` | Root browser session, same-origin, explicit confirmation, expected version | Reject one pending ticket |
| `POST /api/admin/deploy-requests/{id}/dispatch` | Root browser session, same-origin, explicit confirmation, expected version | Retry dispatch only when approval is saved but the worker did not claim the ticket |
| `POST /api/admin/deploy-requests/{id}/retire` | Root browser session, same-origin, explicit confirmation, expected version, non-empty reason | Close an approved ticket only when dispatch definitively failed before a worker was queued; uncertain or running work cannot be retired |
| `POST /api/admin/deploy-requests/{id}/reconcile` | Root browser session, same-origin, explicit confirmation, expected version | Mark a claimed request failed only after its request-specific systemd worker is confirmed stopped; never retries it |

The create payload is a strict DTO containing only repository, branch, full
commit and tree hashes, all configured submodule pins, a GitHub Actions run URL,
and a short summary. The server fixes the allowed repository, branch, and
submodule path set. Unknown fields, incomplete pins, and untrusted workflow URLs
are rejected. Repeated CI submission for a commit with an active ticket returns
that ticket instead of creating duplicates.

## Temporary SSH access

Temporary SSH is a separate request flow, not a side effect of deploy
credentials. The requester starts in Vapor CLI; Identity displays the exact
key fingerprint, purpose, requested capabilities, and duration in a signed-in
browser session. Identity issues a three-minute, single-use
`platform:ssh-access:request` credential bound to the key and a digest of those
request fields. The generic automation-credential UI cannot issue this scope.
Platform verifies the bindings before creating a request. Browser confirmation
only permits request submission; it does not grant access. A root operator
chooses the actual grant from the server allowlist, which may be broader or
narrower than requested, up to the server-wide eight-hour maximum.

| Route | Authority | Purpose |
| --- | --- | --- |
| `POST /api/identity/v1/cli/ssh-authorization/start` | Public, quota-limited request start | Create a short-lived browser-confirmation request; returns the authorization URL and polling code |
| `GET /cli/ssh/authorize?user_code=…` | Public view; sign-in required to confirm | Inspect the exact request before authorizing submission |
| `POST /cli/ssh/authorize` | Signed-in Identity profile, same-origin | Confirm or deny submission of this exact request |
| `POST /api/identity/v1/cli/ssh-authorization/poll` | Device code held by the initiating CLI | Poll status; after confirmation, redeem one single-use credential bound to the exact request |
| `POST /api/ssh-access/requests` | Single-use Identity credential scoped to `platform:ssh-access:request`, bound to key and request digest | Submit the Ed25519 key and the exact authorized request |
| `GET /api/admin/ssh-access/requests?limit=50&cursor=…` | Root browser session | Returns `{ items, next_cursor }` for request and grant state. Page size is 1–200; omit `cursor` for the first page and follow the returned cursor for older records. |
| `GET /api/admin/operations/history?limit=50&cursor=…` | Root browser session | Returns `{ items, next_cursor }` for durable Platform deploy/restore outcomes. Page size is 1–200; omit `cursor` for the first page and follow the returned cursor for older records. |
| `POST /api/admin/ssh-access/requests/{id}/approve` | Root browser session, same-origin, explicit confirmation, expected version | Provision a separate temporary account with the operator-selected policy-allowed grant |
| `POST /api/admin/ssh-access/requests/{id}/reject` | Root browser session, same-origin, explicit confirmation, expected version | Reject a pending request and discard its stored public key |
| `POST /api/admin/ssh-access/grants/{id}/revoke` | Root browser session, same-origin, explicit confirmation, expected version | Remove a grant and terminate its sessions |

Grant capabilities are `vapor_deploy`, `vapor_service_control`, and
`host_admin`; a shell with no sudo is implicit. The operator can reduce or
expand the request within this allowlist and the eight-hour hard limit. The host-admin grant is
exclusive, requires a separate acknowledgement, and gives unrestricted sudo,
so it can disable its own expiry. Lesser grants use fixed wrappers and cannot
choose arbitrary commands or unit names. A per-grant persistent systemd timer
removes the account and key at expiry, including after reboot. The privileged
helper rechecks the Identity root session and the reviewed key fingerprint
before changing host state. The temporary-access installer must be completed
as part of the first guarded cutover before this request UI can issue grants.

## Server-local configuration

`configure-authorization.sh` creates a distinct request-verification token for
Identity and Platform, plus separate root-session introspection credentials.
Only hashes of user automation credentials are persisted by Identity. The
Platform request database is `${VAPOR_STATE_ROOT}/platform/platform.sqlite3`,
owned by the composed Platform service and included in state export/import.

The old GitHub Actions SSH deploy handoff is not part of this flow. CI has no
SSH key or permission to start the systemd deployment unit. Temporary SSH is a
separate, explicitly approved operator facility; it does not authorize CI and
does not replace the exact-revision deploy ticket.
