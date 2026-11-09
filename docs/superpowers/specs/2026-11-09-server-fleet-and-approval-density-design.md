# Server fleet and deployment approval workbench

## Intent

Give operators one compact, accurate place to review deployment decisions and
one separate place to understand the small, manually operated set of machines
that run Vapor. Physical server nodes are different from the independent
service processes already shown by Hosting.

## Goals

- Show the deployment target, exact revision, CI evidence, request age/expiry,
  ordering status, and decision controls without repeating verbose metadata.
- Show private, read-only telemetry for registered server nodes: CPU load,
  memory, uptime, observation time, and node role.
- Keep service health in Hosting and deployment lifecycle in Deployments.
- Establish an outbound-reporting node model that can grow beyond the current
  VPS without exposing a general-purpose remote shell.
- Preserve Platform as the owner of cross-node inventory; service repositories
  keep their own data and business logic.

## Non-goals for this slice

- Arbitrary shell, interactive VNC, or free-form remote commands from the web.
- Pretending a server exists before it has an enrolled identity and telemetry.
- Moving service health checks or service-specific operations into the fleet.
- Adding an external metrics stack or public/player telemetry.
- Automatically provisioning, removing, scaling, scheduling, or repairing
  machines based on a role or metric.

## Approaches considered

1. **Dashboard polls each host over SSH.** Rejected: it couples web requests to
   SSH availability and broad credentials, and risks turning the dashboard into
   a remote shell.
2. **A Platform-owned fleet with node agents reporting outward.** Selected:
   each host has an explicit node identity; a narrow agent reports bounded
   metrics over TLS; Platform stores the last report and exposes typed,
   audited operations later. This fits the existing single-domain gateway and
   independent-service ownership.
3. **Adopt a monitoring suite first.** Deferred: it would add infrastructure
   and setup work before the existing admin flow can present a useful compact
   fleet view.

## Design for this implementation

- Add a root-only Server Fleet page separate from Hosting.
- The current Platform host is the first node. Its metrics are sampled in the
  background and served from a cached snapshot; rendering the page does not
  shell out or trigger per-request probes.
- Use a stable host-derived node identifier and an operator-set label. Expose
  only operator-facing inventory/metrics through the authenticated API.
- Keep CPU percentage, logical CPU count, memory totals/availability, uptime,
  load average, and observation time distinct from service health.
- Compact deployment approvals into a short primary row and collapsible
  evidence details. The action remains bound to the exact current-main SHA and
  validated deployment target; confirmation names both values.
- Server creation means adding a record for a machine the operator already
  provisioned. The operator installs its agent manually. Removal from the
  inventory does not destroy a machine or erase its source/data.
- `control-plane`, `builder`, `backup`, and `general` are descriptive labels
  only. They never route work, start services, or alter a node.
- Any future control is an explicit, typed, operator-triggered action. It must
  be recorded before execution, report a distinct outcome, and never retry an
  uncertain destructive action automatically. It is separate from telemetry.
- No unsupported node is displayed as online.

## Trust and failure behavior

- Fleet data is private and requires the Identity root role.
- If local metrics are unavailable, return an explicit unavailable state rather
  than zeros.
- A stale observation is visibly stale and cannot be mistaken for live health.
- Remote operation authorization is not inferred from telemetry. A typed
  action requires the existing root Identity session and a durable Platform
  operation record; it must not turn the telemetry credential into a control
  credential.

## Acceptance

- The Server Fleet navigation entry is distinct from Hosting and Deployments.
- The API returns cached metrics for the current server and rejects missing or
  non-root sessions.
- CPU/memory/uptime sampling runs on a bounded interval, not per page render.
- Deployment cards expose exact decision facts compactly and preserve stale,
  ordering, target-validation, and current-main protections.
- Rust tests, deployment queue tests, embedded JavaScript parsing, and diff
  checks pass before publishing.
