# Host-First Server Operations Design

## Intent

Make `/admin/servers` a general machine-operations workbench for the small
number of hosts Vapor actually runs on. The current production topology is one
physical VPS. A second host is for recovery and replacement drills, not
autoscaling or a preassigned worker role. Vapor services remain independently
running processes on a host.

The workbench should answer general system-administration questions: is the
host alive, what resources and network activity does it have, what processes
are running, and which typed host actions are available? Product/service
health, publishing, and deployment ownership remain in their service-owned
areas.

## Goals

- Treat physical machines as neutral host records identified by a stable place
  and a display label; do not require `builder`, `backup`, or `control-plane`
  classification.
- Keep the current single-host deployment model explicit and make recovery to
  a replacement host a deliberate operator workflow.
- Run each Vapor service in its own executable and systemd process; remove the
  dormant all-services-in-one-executable runtime path.
- Keep `/admin/servers` focused on generic host facts and safe, typed
  operations: CPU, memory, load, uptime, process inventory, network counters,
  TCP-state counts, service/process controls, activity, and reboot.
- Keep collection bounded and minimize secret exposure: no process command
  arguments, environment values, packet contents, or remote peer addresses.
- Preserve the shared admin gateway while each service retains its own process,
  state, health route, and independent deploy target.

## Non-goals

- Autoscaling, scheduling hosts into roles, or automatically moving services
  between machines.
- Arbitrary shell commands, arbitrary process termination, packet capture, or
  storing payload/peer-level network data.
- Moving service-owned application diagnostics into the generic host view.
- Introducing schema migrations or compatibility with the pre-alpha
  all-in-one runtime path. Only the host-inventory tables may be reset when
  replacing their role-based schema; identity, deployment approvals, SSH
  grants, and operation history are not to be reset by that change.

## Approaches

1. **Keep a dual runtime and annotate it as a legacy option.** Lowest immediate
   cost, but preserves a second deployment/recovery model that no longer
   matches production or the intended independent-service architecture.
2. **Services-only runtime with a host-first operator model (selected).** Keep
   one physical primary host for now, model any replacement as another neutral
   host record, and make the separate service processes the only supported
   runtime. This matches the live deployment and keeps recovery testable.
3. **Split all application services onto separate hosts now.** This would
   confuse process separation with machine scaling, add operational cost, and
   work against the explicit single-host recovery goal.

## Design

### Runtime and deployment

`vapor_platform_server` is only the Platform/admin gateway. Other application
services are separate binaries and systemd units. There is no runtime-mode
switch and no build that links every service router into Platform. Full-stack
deploys still compose all services; scoped deploys continue to update only
their fixed service target.

The server-mode value is removed from new deployment request and operation
contracts where feasible. Where a recovery manifest still needs a topology
marker, it records only `services`; unsupported `monolith` snapshots are
rejected with a direct re-export instruction. Do not reset Platform Identity,
approval, SSH, or server-operation databases to implement this change.

### Physical host model

Host connection records a label, stable host place, credentials, telemetry, and
history. It has no predefined role. Reinstallation may attach a new agent
installation to a vacant existing host place after the previous connection is
revoked. Production remains one host until an operator deliberately adds a
replacement or test machine.

The role-bearing host-inventory tables may be recreated as one deliberate
schema reset. This reset must be limited to enrollment credentials, node
telemetry/history, and network history. It must not touch other tables in the
shared Platform database.

### Host workbench

The selected-machine panels represent generic host administration. Existing
network telemetry and fixed typed service-restart/reboot operations remain.
Add a bounded process inventory from `/proc`: PID, comm/name, process state,
CPU rate, resident memory, UID, and sample time. Never collect argv, cmdline,
environment, executable paths, or file descriptors. Remote agent reports are
size-limited, validated, and rate-limited like existing telemetry.

Vapor service-specific diagnostics and releases remain service-owned. The host
view may show systemd process/resource facts, but it must not become a second
deployment or application-health authority.

## Acceptance

- Builds and systemd install scripts produce independent service executables;
  `src/main.rs` always runs the Platform gateway.
- Deployment requests and recovery reject the obsolete monolith topology; a
  services-mode deployment/recovery remains complete.
- Host enrollment requires no role; host cards, events, and replacement flow
  display no guessed purpose.
- Process inventory is bounded, sorted for useful inspection, and omits
  command arguments, environment values, and executable paths.
- Local and remote hosts expose the same process schema, with explicit stale or
  unavailable states.
- Existing network metrics, machine reboot, typed service restart, and
  activity records continue to work.
- The complete repository test recipe passes; deployment health remains
  independently verifiable.
