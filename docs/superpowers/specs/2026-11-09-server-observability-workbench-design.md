# Server Observability Workbench Design

## Intent

Give the owner one protected place to understand what each Vapor server is
doing, diagnose service and network problems, and collect short-lived evidence
for security investigations. This extends the Server control room without
turning it into a general remote shell or mixing operator telemetry with
player-submitted Diagnostics reports.

## Current Evidence

- Platform owns the selected-server control room, the local `/proc` sampler,
  node inventory, outbound node agent, and typed server operations.
- The node agent currently reports CPU, memory, uptime, and load every 30s;
  the work in progress adds an optional schema-2 network family and bounded
  aggregate trend history. Existing schema-1 agents remain accepted.
- The Diagnostics service explicitly owns opt-in player diagnostics upload,
  storage, retention, and export. Its own instructions prohibit converting it
  into host telemetry or collecting persistent machine identity.
- Live inspection of the core host found two interfaces, zero interface errors
  or drops at that sample, and 304 failed SSH authentication journal entries
  over the prior 24h. The count is evidence of failed attempts, not attribution
  or proof of a targeted attack.
- The separate Ubuntu worker is not enrolled. Its SSH service answered, but the
  workstation rejected the session at host-key verification; the key must be
  verified out of band before login.

## Goals

- Show per-server interface traffic totals and rates, packet/error/drop
  counters, TCP state counts, and bounded recent trends.
- Provide an on-demand connection view with local/remote endpoints and process
  attribution where the host can provide it.
- Surface concise security and service evidence: SSH authentication outcomes,
  service restarts/failures, typed operation history, disk pressure, and
  resource use.
- Offer an explicit, time-limited packet-capture tool for root operators, with
  a fixed maximum duration and byte budget, a required reason, a visible audit
  entry, and automatic expiry.
- Make telemetry useful across the core VPS and outbound-connected Linux nodes;
  collection failure or stale data must be visible rather than presented as
  zero.
- Keep routine controls typed and reproducible. No arbitrary shell command,
  automatic firewall mutation, or automatic attack response is introduced.

## Non-goals

- Do not turn counters or heuristics into a claim of complete intrusion
  detection or future-attack prediction.
- Do not collect game/player diagnostics through this operator telemetry path.
- Do not make packet capture continuous or include capture files in backups.
- Do not introduce a second identity provider, new account system, or new
  authorization authority; reuse the existing root-authorized Identity path.
- Do not expose host metrics publicly or persist detailed peer addresses as
  continuous telemetry.

## Approaches Considered

1. **Aggregate-only telemetry.** Very low risk and simple to retain, but cannot
   answer which live peer or service owns a connection, and cannot preserve
   packet evidence.
2. **One operator diagnostics plane inside Platform.** Platform already owns
   host inventory and typed operations, while the separate Diagnostics service
   is deliberately limited to opt-in user reports. This can combine passive
   trends with explicit, audited on-demand inspection without duplicating
   identity or making another service authority. **Chosen.**
3. **Continuous packet/flow collection in Diagnostics.** Broad visibility,
   but changes the user-report service into a privileged host monitor, creates
   sensitive long-lived traffic records, and violates the current service
   boundary. Rejected.

## Proposed Workbench

Add a `Diagnostics` tab inside each selected server's control room. It has four
focused views:

- **Traffic:** interfaces, current RX/TX rates, cumulative counters, errors and
  drops, TCP state counts, sample freshness, and bounded charts.
- **Connections:** an explicit one-shot snapshot of sockets. Show local/remote
  address, protocol, state, and owning process only where available. Do not
  write peer addresses into periodic telemetry or long-term history.
- **Security:** summarized SSH and service-authentication events over fixed
  windows, plus relevant service and operator-action history. Detailed rows
  require an explicit reveal and remain root-authorized.
- **Capture:** an operator-started capture with a required reason, selected
  interface, structured protocol/port/host filters, a short fixed TTL, and a
  strict maximum file size. Never accept shell text or raw BPF from the browser.
  The capture is held only in volatile storage, never in state exports, and is
  automatically deleted on completion, timeout, cancellation, or service
  restart. Every start/download/cancel is recorded in Platform operation
  history.

## Data and Ownership

- Platform owns collection configuration, node inventory binding, freshness,
  retention, operator authorization, typed operation audit, and rendering.
- The local collector reads bounded kernel counters. The Linux node agent sends
  schema-versioned aggregates to the existing authenticated telemetry endpoint.
- A bounded Platform-owned history stores aggregate samples only (seven-day
  rolling window; cleanup during writes and on startup). It stores at most
  20,160 rows per node and returns at most 1,008 time-bucketed samples per
  request. The latest authorized sample may include bounded interface names
  and counters; history stores only non-loopback aggregate counters and TCP
  state counts. Neither form stores peer addresses or packet content.
- On-demand socket snapshots and capture requests travel through explicit
  typed operations. A remote agent cannot invent or authorize its own action.
- Existing Identity root authorization remains the sole admin decision point.
- The player Diagnostics service remains unchanged and opt-in.

## Safety and Failure Semantics

- Mark each metric family independently as current, stale, unsupported, or
  unavailable; missing measurements are never coerced to zero.
- Bound interface count, connection result count, journal time range, response
  bytes, history rows, capture duration, and capture bytes.
- Do not automatically block IPs, alter firewall rules, or restart services
  based on telemetry.
- Capture is disabled if volatile storage, filter validation, or cleanup
  guarantees cannot be established. A failure never falls back to a persistent
  path.
- Expired or revoked node credentials cannot submit or retrieve diagnostics.
- Detailed connection and capture data is never cached by browsers or proxies.

## Delivery Sequence

1. Add aggregate network metrics, server-side rates/history, and the Traffic
   view for local and enrolled nodes. This implementation is the first slice;
   detailed connection and security panels remain follow-on work.
2. Add typed one-shot connection snapshots and the Connections view.
3. Add bounded SSH/service security summaries and the Security view.
4. Add the volatile, audited packet-capture lifecycle and the Capture view only
   after host capability and cleanup behavior are verified on the disposable
   Ubuntu server.
5. Complete the new-server bootstrap/enrollment/recovery drill and document
   operator-facing limitations and exact recovery steps.

## Acceptance

- A server page shows truthful fresh/stale state, interface rates and errors,
  TCP counts, and recent aggregate trends for local and enrolled Linux hosts.
- Periodic telemetry and its history contain no peer address or payload data.
- Connection snapshots are on-demand, bounded, root-authorized, and not
  retained as telemetry.
- Any packet capture has a required reason, bounded filter/duration/size,
  explicit audited lifecycle, volatile storage, and reliable automatic
  deletion. If these guarantees cannot be implemented and verified, Capture
  remains visibly unavailable rather than silently weakened.
- Player Diagnostics upload/storage behavior and ownership remain unchanged.
- The disposable Ubuntu worker can be installed, connected, inspected, and
  removed/replaced through documented, reproducible steps without requiring a
  public DNS name.
