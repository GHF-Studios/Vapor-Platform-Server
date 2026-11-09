# Per-service Resource Telemetry Design

## Intent

Show the owner how much CPU, memory, and block I/O each Vapor service is using
on the selected server, alongside the existing host-wide metrics. This is
read-only telemetry; it does not grant a service operation or imply health.

## Chosen approach

Read Linux cgroup v2 files directly for a fixed map of known Vapor service IDs
to exact systemd units. Keep sampling in the existing host sampler/node agent,
which already has a 10/30-second cadence. Do not invoke `systemctl` per sample,
walk arbitrary cgroups, add eBPF, or accept unit names from a client. Hosts
without cgroup v2 or without a running unit report that service as unavailable.

## Report shape and bounds

- Each entry has a fixed `service_id`, an availability flag, optional
  `cpu_millicores` for the interval since the previous sample, current memory
  bytes, and cumulative read/write bytes.
- `1000` millicores means one fully occupied CPU core; values may exceed 1000
  for parallel services. The first sample has no CPU rate.
- Service IDs and unit paths are an in-code allowlist. At most 10 entries and
  1 KiB of content per cgroup file are read. Counter overflow, malformed
  values, missing controller files, and resets yield unavailable fields; they
  never become zero-valued success.
- Remote telemetry stores only the latest bounded service snapshot in the
  Platform inventory row. This slice adds no second history table; later
  history must have an explicit retention and sampling design.
- The node reporter credential remains telemetry-only. Service metrics do not
  alter typed-operation capabilities.

## UI

Show a compact resource table in the selected server's Services view: service,
CPU (cores), memory, and read/write I/O totals, with a sample-age label. Missing
services are marked unavailable; no service can be restarted from this table.

## Acceptance

- Cgroup parser tests cover valid/malformed CPU, memory, and I/O data, exact
  unit allowlisting, absent services, oversized files, and counter reset.
- Node telemetry schema 3 requires the complete current report shape, including
  the fixed service snapshot. Schema 1, 2, unknown versions, and unknown fields
  are rejected; this contract has not shipped, so no compatibility window is
  retained. The service snapshot is range-checked and persisted only with an
  accepted fresh report.
- Root-only fleet response includes the cached service metrics and remains
  `private, no-store`; schema-1/2 reports are rejected. Deploy the Platform
  server and node agent as one versioned unit.
- UI rendering is text-only, bounded, and states unavailable/stale data
  honestly. Existing health checks and typed actions remain separate.
