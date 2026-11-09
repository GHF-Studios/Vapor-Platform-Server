# Server operator workbench redesign

## Intent

Replace the current “fleet list plus enrollment form” with a host-first
operator workbench. The owner already manages a small number of VPS machines
with Strato. Vapor should make it easy to inspect each machine and use common
server tools without reaching for raw SSH for every task.

The TeamViewer comparison is about convenient remote operation from one
workbench. It is explicitly **not** a remote desktop. The operator should see
metrics and server tools, invoke typed actions, inspect their results, and use
the separate emergency browser shell only for work the typed tools do not yet
cover.

## Goals

- Make a selected server the organizing object, not its credential or
  enrollment record.
- Let the owner switch between known machines and see health, freshness, and
  host resource readings for the selected machine.
- Offer a compact panel of tools: Overview, Services, Access, Activity,
  Console, and Connections.
- Make service/module operations and their availability host-specific.
- Provide direct access to the existing SSH authorization manager and the
  separately gated emergency console without merging their security models.
- Move add/reconnect into Connections as a guided task for a machine already
  provisioned elsewhere. Secrets appear only at its explicit completion step.
- Prefer typed server operations as the future routine-work surface; preserve
  the browser terminal as a narrow temporary escape hatch.

## Non-goals for this first redesign slice

- Do not pretend service process/cgroup resource readings exist: the node agent
  currently reports host-level CPU, memory, uptime, and load only.
- Do not claim remote SSH authorization inspection or remote shell control:
  those currently belong to the core host and separate routes.
- Do not implement service movement/rebuild flows, Strato APIs, automatic
  provisioning, autoscaling, or general-purpose remote command execution in
  the page redesign.
- Do not invent dead buttons for unsupported tools. Show explicit capability
  limits and link to a real implementation when one exists.

## Approaches

1. Rename the current stacked page. Rejected: same mental model and clutter.
2. Make a host-first control room with a machine switcher and tool tiles
   (selected). It makes the machine and the operator's next task obvious while
   keeping current service-owned and security boundaries intact.
3. Build an all-in-one remote shell/management daemon first. Rejected: it
   combines unrelated privileges, adds a large security surface, and delays
   the useful typed tools already supported.

## Screen model

### Host switcher

Show the core host and any manually connected hosts as compact selectable
cards. Each card shows only known state, role, last contact, CPU/memory/load,
and whether typed actions are currently reachable. Selecting a host changes
the context of the tools below. Internal node and slot IDs remain in an
advanced disclosure.

### Selected-host header

Display the human label, operational state, role, and latest sample time. Do
not combine host health with service health, and do not call a configured but
silent host “online”.

### Tool tiles and workspace

Provide a tight tool palette for Overview, Services, Access, Activity, Console,
and Connections. Selecting a tile replaces the focused workspace panel; it
does not navigate to a different security universe.

- **Overview:** host-level CPU, memory, uptime, and load with freshness.
- **Services:** existing typed service restart and reboot actions, scoped to
  the selected host and capability state; link to the current Hosting view for
  service health. State that per-service resource data is not collected yet.
- **Access:** direct link to the SSH grant workbench, accurately labeled as
  core-host scoped until remote SSH management exists.
- **Activity:** recent typed actions filtered/displayed with their target host.
- **Console:** link to the separate time-limited core-host emergency console.
- **Connections:** add a machine or connect a replacement into a vacant slot.
  This is the only place that creates a node record or reveals one-time
  credentials.

### Connection task

Visible actions are “Add server” and “Reconnect replacement”, not “enroll”.
The flow makes clear that the VPS is provisioned through its provider first.
The operator identifies a new or vacant logical slot, asks Vapor to prepare a
connection, downloads the generated agent config, installs the signed/tested
agent package, and waits for its first report. The config download is primary;
raw secrets are opt-in and clear on close/page exit.

## Invariants

- The page is a navigator and composition surface; each API still enforces its
  own authorization, especially SSH and emergency-console operations.
- A visible tool tile must open a functioning page or panel; unsupported tools
  are described as gaps, not faked.
- Host telemetry, service health, and executor availability are separate facts.
- Remote actions are exposed only for a recently connected executor.
- Credentials do not dominate the main page and remain one-time secrets.
- No secret, arbitrary command, filesystem path, systemd unit string, or
  provider credential comes from page input.

## Acceptance

- The primary page is host-first and contains compact host selection, status,
  and tool tiles; it no longer reads as an enrollment form.
- Choosing a host scopes real typed actions to that host, with unsupported
  actions visibly unavailable.
- Services, Access, Activity, Console, and Connections are reachable from the
  same workbench and lead to the existing authorized capability owners.
- Add/reconnect is staged, says the server must already exist, and presents a
  download-first one-time config flow.
- Per-service resource readings are explicitly marked unavailable until the
  agent/API implements them.
