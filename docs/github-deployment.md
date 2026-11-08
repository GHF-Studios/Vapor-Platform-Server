# GitHub-controlled deployment

GitHub Actions runs the composed CI workflow and, only after CI succeeds,
submits an exact-revision deploy request. It never receives SSH access to the
VPS and never approves or starts a deployment.

## Flow

1. CI validates the Platform repository and every pinned root-level service
   submodule.
2. The request workflow submits the full root commit, tree, pinned submodule
   revisions, CI run URL, and resolved deployment target to Platform using an
   Identity credential scoped only to `platform:deploy-request:create`.
3. A root operator reviews the exact source and deployment target in
   `/admin/operations` and approves or rejects it. Legacy requests without a
   recorded target cannot be approved; submit a fresh CI request instead.
4. Approval immediately starts the systemd unit for that request ID. The
   worker revalidates the immutable request and executes only the approved
   revision and target. It does not infer topology from mutable host settings.
5. The Operations dashboard records the result. A dispatch failure before the
   worker claims the request can be retried by a root operator. If a worker
   stops after claiming the request, a root operator can reconcile it only
   after the dashboard helper confirms that request-specific systemd unit is
   inactive or failed. Reconciliation records terminal failure and never
   replays a possibly partial deployment; a new CI request and approval are
   required.

## Target selection

Deployment-relevant pushes to `main` run the composed CI before creating a
request. Changes to one root service submodule select only that service (or
`atlas` for the static Atlas site). Changes to Platform runtime/deployment
inputs, or changes to more than one service submodule, select the full
composition. Unknown root paths also select the full composition so new runtime
inputs cannot be silently skipped. Root documentation, Markdown files,
workflow files, and tests do not request a VPS deployment.
`workflow_dispatch` keeps its explicit target selector for an operator-requested
full or single-service build.

There is no deploy polling timer. The independent state-export timer remains.
Temporary SSH access is a separate, explicitly approved capability managed in
the same Operations dashboard; CI credentials cannot request or grant SSH.

## GitHub configuration

On `GHF-Studios/Vapor-Platform-Server`, configure:

- Secret `VAPOR_DEPLOY_REQUEST_TOKEN`: an Identity credential with only
  **Create Platform deploy requests** selected.
- Variable `VAPOR_DEPLOY_PUBLIC_BASE`: `https://vapor.ghf-studios.site`, with
  no trailing slash.

If either value is absent, CI reports that it skipped request creation. It
does not fall back to SSH, auto-deploy, or any unapproved deployment path.

The request-verification service token and root-session introspection
credentials remain in root-readable VPS env files. They are separate from the
GitHub request credential and are never uploaded to Actions.

## First install and migration

The first Platform install or a migration from the preserved previous server
layout is an explicit root operation, not an Actions job. Keep `/opt/vapor-platform-server`,
service state, `/etc/vapor-server`, and the active Caddy configuration backed
up until the new approval dashboard and health checks are verified. The
polling deploy timer must remain disabled; migration and restore scripts must
not reactivate it.

## Verification

For a new request, confirm that Actions records a pending request, the root
dashboard shows the exact commit/tree/submodule pins, and no deployment starts
before approval. After approval, the request-specific unit should run once and
record success or failure. Public health checks and the Build Cache inventory
are part of the post-deploy check. Build Cache snapshot availability must be
reported per target; a healthy endpoint alone does not mean a signed target
snapshot or the VPS-hosted target builders are ready.
