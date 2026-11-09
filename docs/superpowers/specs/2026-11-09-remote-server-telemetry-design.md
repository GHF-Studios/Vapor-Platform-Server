# Remote server telemetry and enrollment

## Intent

Extend the private Server Fleet from a truthful view of the Platform VPS into
a real inventory that can receive bounded telemetry from separately enrolled
servers. The next expected node is a build or backup worker, but the first
version must not assume that one exists.

## Approaches considered

1. Platform polls remote hosts over SSH. Rejected: it couples page/backend
   availability to SSH and gives the web application a broad credential that
   could drift into remote command execution.
2. Enrolled hosts push typed telemetry over HTTPS using a per-node secret.
   Selected: only the node agent can report metrics; root operators manage
   enrollment and revocation; host control remains a separate capability.
3. Install a third-party monitoring suite first. Deferred: it adds deployment
   and maintenance cost before the first optional node is proven useful.

## Goals

- Root operators can create a named node record, choose its operational role,
  receive a one-time reporter credential, and revoke that credential.
- A small Linux reporter sends only CPU, memory, uptime, load, and agent version
  over HTTPS to the Platform origin.
- Platform stores the reporter credential only as a digest, persists node
  inventory and the latest sample in Platform-owned state, and presents stale
  or unavailable reports honestly.
- Metrics ingestion is bounded, validated, rate-limited per credential, and
  cannot dispatch commands or grant SSH access.
- The Platform node remains locally sampled. Remote-node telemetry is additive
  and does not replace Hosting's service-health view.

## Non-goals

- Web SSH/VNC, arbitrary commands, process lists, disk file browsing, or service
  control from this feature.
- Public telemetry, user/player telemetry, or identifying a node by its
  hostname or machine-id without operator enrollment.
- Automatic node creation or assuming worker capacity exists.
- Replacing the future native-builder lifecycle or Build Cache ownership.

## Design

- Platform owns a `platform_server_nodes` table in its existing state database.
  Node identity, label, role, lifecycle state, credential digest, creation and
  revocation times, and latest validated sample are stored there. No other
  service reads this database.
- A `platform_server_node_events` table records node creation and revocation,
  actor profile ID, node ID, event time, and the non-secret label/role snapshot.
  The event log does not copy reporter credentials or metric payloads.
- Root-only create/list/revoke routes are served by Platform. Creation returns
  the generated node ID and reporter secret once with `Cache-Control:
  no-store`; the server stores only a SHA-256 digest. The browser keeps the
  secret only in the one-time response view (never local/session storage) and
  clears it when that view is closed. The credential is bound to that node and
  grants only the `report_telemetry` action. Revocation takes effect on the
  next report.
- A reporter credential contains a random node identifier and a high-entropy
  secret. The reporter sends it only over HTTPS to a fixed configured Platform
  origin. Requests have a small body limit and strict schema; accepted reports
  require finite/range-checked metrics and a sample time no more than 90
  seconds old or 60 seconds in the future. Platform records its own receipt
  time and atomically accepts a report only when at least 10 seconds have
  passed since the prior accepted report and the sample time is strictly newer
  than the stored sample.
- The reporter is a standalone Linux binary in this repository's Platform
  tooling. It reads `/proc`, samples on a fixed interval, and sends no data
  other than the typed metric payload. It supports a foreground one-shot mode
  for setup validation and a service mode for later systemd installation.
- Admin UI exposes explicit enrollment, role/label, one-time secret display,
  revoke, reporting status, and sample age. It must never render the secret
  again after creation. Fleet status remains separate from service health.
- First roles are `control-plane`, `builder`, `backup`, and `general`; these are
  descriptive inventory labels, not access grants or scheduling authority.
- Labels are trimmed, 1–80 Unicode scalar values, and reject control
  characters. Role values are an exact allowlist; arbitrary role text is not
  persisted.
- The reporter runs unprivileged as `vapor-node-agent`. Its config is
  `/etc/vapor/node-agent.toml`, owned by `root:vapor-node-agent` with mode
  `0640`; only the agent's dedicated group can read its node secret. No shell
  interpolation or environment-variable expansion is performed on config
  values.

## Trust and failure behavior

- Reporter secrets are generated with operating-system cryptographic
  randomness, never logged, and never returned by list/read endpoints.
- Node create/revoke events are attributed to the authenticated root profile.
  Reporter attempts are not logged with bearer credentials or full metric
  bodies.
- A secret is scoped to one node and telemetry submission only. It does not
  authorize enrollment, another node, shell access, service control, or
  deployment.
- Revoked, malformed, oversized, stale, future-dated, out-of-order, or
  non-finite reports are rejected without mutating the latest accepted sample.
- Node state is `online` only while both last receipt and sample are within the
  90-second freshness window; older reports display `stale`, and revoked nodes
  display `revoked` regardless of their last sample.
- Admin routes require an Identity root session. Telemetry reports require the
  node credential and are rate-limited independently of browser authorization.
- The reporter uses a fixed HTTPS origin, refuses redirects, validates TLS,
  and has bounded connect/request timeouts.
- The configured endpoint must be a bare HTTPS origin with no credentials,
  path, query, or fragment. The reporter refuses redirects and does not allow
  an operator-supplied origin to be changed by a report response.

## Acceptance

- A root operator can create and revoke a remote node, and sees the secret only
  in the create response.
- A valid enrolled node can report metrics; the private fleet page shows its
  role and receipt age without listing services or exposing host internals.
- Bad credentials and invalid payloads do not update stored telemetry; revoked
  credentials stop reporting immediately.
- Creation/revocation is auditable to the root profile; secret-bearing create
  responses are non-cacheable and the secret is not persisted in browser
  storage.
- Stale and revoked states are distinct, and the local Platform node remains
  available when remote nodes are offline.
- No endpoint in this feature executes a command or accepts arbitrary
  operation payloads.

## Builder decisions

- Use the existing Platform SQLite database and Axum router rather than a new
  service; fleet inventory and enrollment are Platform orchestration state.
- Keep the first reporter Linux-only because the optional worker target is
  Linux; add another reporter platform only when a real supported node needs it.
- Set the initial reporting interval to 30 seconds and online freshness window
  to 90 seconds, with limits centralized for later adjustment.
