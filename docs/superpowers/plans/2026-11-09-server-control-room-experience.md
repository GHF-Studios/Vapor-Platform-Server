# Server operator workbench implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the enrollment-led Servers page with a host-first operator workbench.

**Architecture:** Keep typed operations, SSH management, emergency console, and node connection as separate capability owners. The page selects a host and switches a focused tool workspace, linking to existing owners where a capability is a separate route. Do not add unsupported backend claims.

**Tech Stack:** Rust server-rendered HTML, existing CSS and embedded JavaScript, Axum APIs.

**Spec:** `docs/superpowers/specs/2026-11-09-server-control-room-experience-design.md`

## Global Constraints

- No shell commands, arbitrary units/paths, or provider credentials from browser input.
- Existing route-level authorization remains authoritative.
- Do not fabricate per-service readings or remote SSH/console support.
- Host records and one-time secrets remain distinct; connection is not authorization.
- Preserve unrelated working-tree edits and stage only this feature.

## Review Focus

- Keyboard switching and screen-reader tab state: test tab roles, arrow handling, selected state, and one visible panel.
- Remote/core action scoping: keep executor freshness and core-only restrictions unchanged.
- Secret exposure: prove config and raw values are hidden until the Connection tool and explicit reveal.
- Degraded inventory: keep errors and stale/never-reported states truthful.
- Link boundaries: SSH and emergency console still link to separately authorized owners.

---

### Task 1: Replace the fleet list composition with a host-first control room

**Files:** `src/admin.rs`

- [x] Add selectable host cards with human-readable state, role, freshness, and available host-level metrics.
- [x] Add a selected-host header and a compact tool palette: Overview, Services, Access, Activity, Console, Connections.
- [x] Switch a focused panel on tool selection; support keyboard navigation and accessible selected state.
- [x] Keep unknown IDs and internal incarnation details behind an advanced disclosure.

### Task 2: Wire tools to existing capabilities

**Files:** `src/admin.rs`

- [x] Scope service restart/reboot controls to selected host, preserving current action gating.
- [x] Add direct Hosting, SSH Access, and emergency Console bridges with core-only scope made explicit.
- [x] Move action history into Activity and show its target host.
- [x] State per-service resource metrics are not collected yet; do not reuse host totals.

### Task 3: Make Connections a guided setup task

**Files:** `src/admin.rs`

- [x] Put add/reconnect buttons only in Connections; hide the form until an option is chosen.
- [x] Replace “enroll” copy with machine connection/setup language and explain provider provisioning is external.
- [x] Keep existing create-node and vacant-slot API invariants unchanged.
- [x] Make the config download primary and raw credential reveal deliberate; clear secrets on close/page exit.

### Task 4: Pin behavior and verify

**Files:** `src/admin.rs`, this plan and the design spec.

- [x] Add/update tests for page hierarchy, tab behavior hooks, links, truthful metrics limitations, and hidden credentials.
- [x] Run the full Platform test suite (132 passing), embedded-JS syntax validation, and `git diff --check`.
- [x] Inspect final diff to ensure no backend authorization or unrelated working-tree changes were altered.
- [ ] Commit/push only the reviewed UI slice; wait for Linux CI before deployment.
