# Standalone Vapor Product and Platform Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver Vapor as an independently installed platform that manages Loo Cast and user-owned Mad Max installs under clear source, publication, update, and operator-control boundaries.

**Architecture:** This is a coordinating plan for the approved cross-repo design, not a replacement for the service-owned implementation plans listed below. Execute dependency lanes in order, preserve each repository’s ownership, and require a usable acceptance result at each phase before advancing. Vapor Client owns local install/source workflows; server submodules own their policy and business logic; Platform composes and operates those services.

**Tech Stack:** Rust workspaces and Cargo, Steam depot/launch metadata, signed Vapor release artifacts, Rust/Axum/SQLite service modules, OpenSSH/systemd for VPS operations, and an optional target-build worker with a supported local fallback.

**Spec:** `docs/superpowers/specs/2026-11-03-vapor-standalone-product-platform-design.md`

> **Current Steam packaging decision (2026-11-09):** The older optional
> locator/forwarder plan below is superseded by the
> [2026-11-08 Platform and Product Rebuild Charter](../specs/2026-11-08-vapor-platform-and-product-rebuild-charter.md):
> one self-contained executable artifact embeds the manifest and all three
> first-party products; no separate locator is shipped.

## Global Constraints

- Vapor is installed, updated, repaired, and uninstalled independently of Steam.
- Vapor servers never distribute Loo Cast or Mad Max game files.
- Product/source rights are enforced by server-owned policy; client manifests and UI cannot grant publication rights.
- Keep each service’s business logic and state in its root-level submodule; Platform owns composition and deployment orchestration.
- Preserve independent service rebuild/deploy and composed whole-system export/import.
- Preserve dirty work in Vapor Client, Vapor-Client-madmax, and MM-Building-System; reconcile ownership before combining overlapping changes.
- Use the active Vapor Context/Subject/Selection and source-identity contracts; do not mint parallel product identity schemas.
- Keep the VPS admin toolbench distinct from the Vapor Client’s local game workflows.
- Keep secrets out of source, ordinary state exports, logs, and public artifact feeds.
- Keep the existing exact-revision deploy approval contract and service-owned Identity permission approval contract.
- Keep one current signed build snapshot per supported target and rolling lock. A second VPS worker is optional; source builds and service deployment retain a local fallback when it is absent. See [`../vapor-build-snapshot-contract.md`](../vapor-build-snapshot-contract.md) for the current boundary and implementation status.
- Install Vapor independently under the user's Vapor home (`%USERPROFILE%\Vapor` or `$HOME/Vapor`); Steam must not own Vapor application files, tools, caches, configuration, or state.
- The only local setup roles are `Player` and `Developer`. Developer setup always includes the development toolset, both supported targets, and managed cross-compilers; it cannot be skipped. Server-side ecosystem authorization is separate from these local roles.
- The Loo Cast Steam target is one self-contained executable artifact embedding `App.vapor.toml`, Engine, Game, and Packagepack. A separate locator/forwarder is not part of the depot payload; the exact Steam-start behavior of the artifact remains open.
- Mad Max maps to Vapor's layers: locally acquired `MadMax.exe`/required original runtime files are the Engine source; scripts/content are the Game layer; `MadMaxModding` is the integration Packagepack that manages ASI-loader and bridge setup so Vapor content can participate. The Packagepack never contains Mad Max game files.

## Review Focus

- **Stale or conflicting product identity:** a local source must resolve to exactly one canonical product/source identity using Vapor core’s active contract.
- **Manifest spoofing:** an untrusted package claiming “publishable” must still be denied by Identity/Registry policy.
- **Missing or changed local game files:** discovery, cache generation, and repair must never fetch licensed game bytes from Vapor infrastructure.
- **Interrupted update or profile switch:** local state and game installation must recover to a known prior state or a truthful repair-required state.
- **Expired or partially applied VPS access:** the operator workflow must show actual grant state, expiry, audit actor, and cleanup outcome; a submitted or approved request alone must not be reported as active access.

## Repository and Plan Map

The root Platform repository coordinates deployment and owns this roadmap. Detailed implementation remains in its existing service/client plans:

| Workstream | Owner/repository | Existing execution plan |
| --- | --- | --- |
| Canonical Context/Subject/Selection and source identity | `GHF-Studios/Vapor` | Vapor issues #1 and #2; consume their accepted contracts |
| Standalone signed app release and update | `GHF-Studios/Vapor-Client` | `docs/superpowers/plans/2026-11-01-vapor-standalone-distribution-and-updates.md` in the client workspace |
| Server-owned product authority and uploads | Registry, Identity, Artifact submodules | `docs/superpowers/plans/2026-11-02-vapor-server-product-authority-and-uploads.md` |
| Admin shell and operator sections | Platform root + service submodules | `docs/superpowers/plans/2026-11-02-vapor-platform-admin-toolbench.md` |
| Deployment and temporary SSH | Platform root + Identity submodule | `docs/superpowers/plans/2026-11-03-vps-operations-and-temporary-ssh.md` |
| Typed local VPS SSH operation | Platform root | `docs/superpowers/specs/2026-11-03-typed-vps-ssh-operations-design.md` (owner-authorized bootstrap path; web approval remains supported) |
| Target build snapshots and guests | Build Cache submodule + Platform | `docs/superpowers/plans/2026-11-02-vapor-target-build-snapshots-and-builders.md` |
| Mad Max content discovery and mounting | `GHF-Studios/MM-Building-System` + Vapor Client adapter | `docs/superpowers/plans/2026-11-01-madmax-local-data-workbench.md` in the Mad Max workspace |
| Loo Cast product/depot packaging | Loo Cast repository + Vapor Client adapter | Create a focused Loo Cast migration plan after depot/app identifiers and retirement sequence are verified |

**Platform-owned coordination files:**

- Modify: `docs/superpowers/specs/2026-11-03-vapor-standalone-product-platform-design.md` only when an owner-approved architectural decision changes.
- Create: this coordinating plan.
- Existing detailed Platform plans remain authoritative for Platform-owned implementation; do not duplicate their task lists here.

## Immediate First Sprint: Two Parallel Lanes

These two lanes are approved to proceed in parallel because they have different
owners and different users. The website serves players and developers; the VPS
admin toolbench serves platform operators. Do not merge their navigation,
permissions, or operational data into one UI.

### Public Vapor download and first-run journey

- Owner: Vapor Homepage Server for public pages; Artifact Server for signed
  application releases; Vapor Client for local install/update behavior.
- Deliver a polished `/downloads` and getting-started flow for the standalone
  Vapor installer. Explain Player versus Developer setup and what Developer
  installs. Show only real signed releases and supported targets; if a target
  does not yet have a release, state that clearly rather than showing a fake
  download.
- Explain that Loo Cast and Mad Max game files are acquired locally through
  their respective sources and are not distributed by Vapor. Explain that
  Steam provides one self-contained Loo Cast artifact and does not install or
  own the standalone Vapor runtime.
- Keep registration, identity, and local install actions at their actual
  owning surfaces; the public page must not pretend to install or authorize a
  product itself.
- Acceptance: a new user can identify the supported OS/release, download only a
  real signed Vapor artifact, verify what to do next, and understand when a
  product source must be installed locally.

### VPS operator admin toolbench

- Owner: Platform for the shared shell and cross-service operations; each
  service submodule for its own policy, data, state-changing actions, and audit.
- Continue the approved server-rendered shell and service-owned workbenches.
  Show truthful readiness/unavailable states, make the current deployment and
  approval state understandable, and keep operations task-oriented.
- This lane may not turn into a browser terminal, generic filesystem/SQL editor,
  or a second implementation of Identity/Registry/Artifact/Build Cache policy.
- Acceptance: an operator can understand service health and perform only
  implemented, authorized, auditable workflows, with exact target and outcome.

### Parallel-lane boundary

- Each lane works in its owning repository/worktree and prepares its own review
  and commit. Platform pins a service revision only after that service change
  is reviewed and merged/published according to its repository workflow.
- Existing dirty Vapor Client, Mad Max, and Loo Cast work remains preserved;
  neither lane may sweep those changes into its commit.
- No production deployment occurs as part of local implementation. Deployment
  remains a separate exact-revision, owner-approved operation.

## Task 1: Establish an accurate recovery and access baseline

**Files:**
- Read: `docs/superpowers/plans/2026-11-03-vps-operations-and-temporary-ssh.md`
- Read: `docs/superpowers/specs/2026-11-03-typed-vps-ssh-operations-design.md`
- Modify: Platform/Identity operation code only through the approved access plan.

**Interfaces:**
- Input: the existing requester-initiated SSH request and Identity/Platform authorization records.
- Output: an auditable operator path that can inspect the request and report whether a grant is actually active, including exact scope, account, host, and expiry.
- A root-only typed local operator operation is authorized by the owner as the bootstrap path. It must use the existing managed grant/account/expiry implementation, preserve audit attribution, require interactive confirmation after displaying all grant details, and never grant more than the selected allowed scope. Do not create a fallback static key allowlist or edit `authorized_keys` manually.

- [ ] Inspect the live request state through the existing Identity/Platform APIs and identify why web Operations does not display a reliable result.
- [x] Repair the SSH section's authorization/error display locally; make 401, 403, service errors, and bounded response details distinguishable. Remaining service-owned approval/status and deploy work stays in the existing temporary SSH plan.
- [x] Implement the typed `ssh show`, `ssh grant`, `ssh revoke`, and read-only reconfigure guidance in the existing root helper, with local-operator audit identity; require typing the displayed fingerprint for grants and require root plus confirmation for every mutation. Pending-request TTL is enforced before prompting and during the grant transition.
- [ ] Grant through either the browser root decision or typed local operator path only after it displays the exact key fingerprint, selected capabilities, and expiry.
- [ ] Verify SSH authentication, server-side capability scope, audit actor, revoke, and natural expiry before treating access as recovered.
- [ ] Run the SSH and deploy approval regression checks in the existing temporary SSH plan; expected result is an active grant only after the grant operation succeeds and an inactive grant after revoke/expiry.

## Task 2: Reconcile repository ownership without losing work

**Files:**
- Read: Vapor Client main and Mad Max client workstream registries/manifests.
- Read: all dirty files and submodule states in the Vapor Client, Vapor-Client-madmax, and MM-Building-System checkouts.
- Modify: workstream ownership/registry documents only after each change has a named authoritative checkout.

**Interfaces:**
- Consumes: existing dirty trees as source material; no checkout is assumed disposable.
- Produces: a per-change ownership ledger naming source checkout, destination owner, dependency, and merge method.

- [x] Inventory uncommitted changes and submodule revisions; record hashes/status without cleaning or resetting. Snapshot: `.superpowers/sdd/2026-11-03-vapor-standalone-product-platform/repository-ownership.md`.
- [x] Resolve overlapping claims for `Vapor/**`, `Vapor-Installer/**`, CLI SSH support, and Mad Max integration with a written ownership map. Ambiguous update code is preserved and assigned for one canonical integration path before merge.
- [ ] Confirm each destination repository has a focused implementation plan before moving code across repository boundaries.
- [ ] Verify all original dirty changes remain recoverable after any later integration; no broad merge or cleanup is permitted in this task.

## Task 3: Complete Vapor’s canonical identity prerequisites

**Files:**
- Owner: `GHF-Studios/Vapor` core repository.
- Reference: active issues #1 and #2 and their accepted design/implementation artifacts.

**Interfaces:**
- Produces: canonical Context/Subject/Selection and hierarchical source identity contracts for client and product adapters.
- Consumers: Vapor Client, Loo Cast adapter, Mad Max adapter, Registry, Identity, and Artifact.

- [ ] Complete Vapor issue #1 and publish its stable public types/serialization contract.
- [ ] Complete Vapor issue #2 against issue #1 and publish source identity semantics.
- [ ] Add compatibility fixtures proving product source identifiers remain stable across local path changes and distinct products cannot alias.
- [ ] Run Vapor core’s unit, serialization, and dependent workspace checks; expected result is the same identity mapping through local client and server adapters.
- [ ] Update dependent plans to reference the accepted API instead of duplicating assumptions.

## Task 4: Enforce product/source policy on the server

**Files:**
- Modify: `Vapor-Registry-Server` product/source policy and catalog validation.
- Modify: `Vapor-Identity-Server` authorization policy and audited permission path.
- Modify: `Vapor-Artifact-Server` only for Vapor application release artifacts.
- Reference: `docs/superpowers/plans/2026-11-02-vapor-server-product-authority-and-uploads.md`.

**Interfaces:**
- Input: canonical Vapor product/source identity and authenticated author/request identity.
- Output: server-owned decision for local-only, externally licensed, or community-publishable content; signed Vapor executable release remains a separate artifact class.

- [ ] Implement the detailed authority/upload plan after Task 3 supplies canonical identity.
- [ ] Add policy cases for Loo Cast depot-only first-party products, Mad Max licensed local source files, user-created mod content, and Vapor app releases.
- [ ] Verify a client-supplied publishability flag cannot override policy, and that game files are rejected before upload storage.
- [ ] Run Registry/Identity/Artifact policy tests from the detailed plan; expected result is only explicitly authorized community data being publishable.

## Task 5: Complete standalone Vapor installation and release lifecycle

**Files:**
- Owner: `GHF-Studios/Vapor-Client` and its Installer/Core submodules.
- Reference: `docs/superpowers/plans/2026-11-01-vapor-standalone-distribution-and-updates.md` in the client workspace.

**Interfaces:**
- Input: signed Vapor release from Artifact and active platform/toolchain lock.
- Output: independent install, update, repair, rollback, and uninstall under a coherent Vapor home, without Steam owning Vapor files or lifecycle.

- [ ] Reconcile ownership from Task 2 before implementing overlapping updater/installer files.
- [ ] Complete signed archive verification, staged extraction, transactional app-root replacement, rollback, GUI state, and production trust provisioning using the detailed plan.
- [ ] Place managed sources/tools/cache/state under Vapor home according to existing path ownership; do not create a second competing home-layout authority.
- [ ] Ensure Developer setup installs the complete mandatory toolset and Player setup does not expose a skip/partial-Developer route.
- [ ] Make the public download page acquire the signed standalone installer from Artifact and show the supported release/target.
- [ ] Run release signature, interrupted install/update, rollback, uninstall, and clean-machine bootstrap checks; expected result is no Steam dependency for Vapor’s own lifecycle.

## Task 6: Move Loo Cast and Mad Max into distinct managed product adapters

**Files:**
- Modify: Loo Cast product manifests and Steam depot/entrypoint metadata in the Loo Cast owner repository.
- Modify: Vapor Client product source/adapters and launch selection.
- Modify: `GHF-Studios/MM-Building-System` ecosystem manifest/adapter integration only in its own checkout.
- Create: Loo Cast depot/product migration plan after identifiers are verified.

**Interfaces:**
- Loo Cast source: trusted local depot mounting one self-contained executable artifact with embedded manifest, Engine, Game, and Packagepack; no separate locator is included.
- Mad Max source: user-selected local game installation; source files are never uploaded; derived index/cache is stored separately and can be rebuilt.
- Mad Max layers: Engine source is the local executable/runtime; Game is the script/content and installed-mod composition; `MadMaxModding` Packagepack owns the loader/bridge compatibility lifecycle. Mods may target the Engine, Game, or other mods/Packagepacks using Vapor's canonical dependency/conflict/override semantics.
- Output: product/source records conforming to Task 3’s canonical identity contract.

- [ ] Verify Loo Cast Steam AppID/depot IDs, launch choices, existing Workshop item IDs, and owner-controlled retirement sequence; do not infer replacement identifiers.
- [ ] Move Loo Cast first-party content out of public Workshop semantics and mark it server-authoritatively local-only.
- [ ] Keep the official Steam app's launch behavior within the single self-contained executable artifact; any Vapor tools-selector handoff must not bundle, install, update, or own Vapor. Missing Vapor shows install guidance.
- [ ] Add Mad Max install discovery, local source validation, separate derived cache, and reversible mod receipts using the Mad Max detailed plan.
- [ ] Implement `MadMaxModding` as the integration Packagepack: install/check/repair the ASI loader and Mad Max bridge, record their versions and provenance, and make the game receptive to Vapor-managed content. Keep game files out of the Packagepack; acquire third-party loader files from an authorized source if redistribution is not allowed.
- [ ] Map mod targets to Engine, Game, and other installed mods/Packagepacks through Vapor's canonical identity and dependency/conflict/override contracts; do not add a Mad Max-only parallel manifest schema.
- [ ] Verify missing/changed game files lead to actionable repair, never a download from Vapor; verify Loo Cast depot content cannot publish.
- [ ] Test both adapters against canonical identity fixtures and test the Loo Cast launch handoff with Vapor present and absent.

## Task 7: Make the admin toolbench reflect real service operations

**Files:**
- Modify: `src/admin.rs`, Platform routes/auth clients, and shared admin assets.
- Modify: service-owned sections in Registry, Identity, Artifact, and Build Cache submodules.
- Reference: `docs/superpowers/plans/2026-11-02-vapor-platform-admin-toolbench.md` plus its productization phase.

**Interfaces:**
- Platform owns shared navigation, truthful module readiness, cross-service composition, and Platform operations.
- Each service owns its policy, data, actions, and audit result.
- Dashboard actions return operation IDs and expose exact target, confirmation, progress, and final outcome.

- [ ] Complete Task 1 before labeling SSH grants usable; display pending, approved, active, expired, revoked, and failed distinctly.
- [ ] Replace duplicated Rust/JavaScript navigation and hardcoded availability with one route/module registry and service readiness inputs.
- [ ] Split Operations into task-focused Deployments, Temporary Access, Hosting, and History sections while retaining exact-revision deploy approval.
- [ ] Add service-owned catalog, identity, releases, and build snapshot sections through their APIs rather than duplicating business logic in Platform.
- [ ] Preserve useful error bodies, request IDs, audit attribution, and truthful unavailable states in all mutation/list failures.
- [ ] Run section authorization tests, state-transition tests, navigation/readiness rendering checks, and operation error display checks; expected result is each action is owned and audited by its responsible service.

## Task 8: Produce signed rolling target snapshots with an optional worker

**Files:**
- Owner: `Vapor-Build-Cache-Server` and Platform orchestration.
- Contract: `docs/vapor-build-snapshot-contract.md`.

**Interfaces:**
- Targets: `x86_64-unknown-linux-gnu` and `x86_64-pc-windows-gnullvm`.
- Output: one current signed snapshot per target and lock, fetched by Vapor Client after toolchain installation.
- Build location: optional second VPS worker when provisioned and verified; local source builds and service deployment remain available without it.

- [ ] Keep the primary VPS as the sole public/policy authority; do not install virtualization or assume it can host build guests.
- [ ] After the optional second node is selected, validate its OS, CPU/RAM/disk, network, isolation, and Windows toolchain licensing before provisioning.
- [ ] Implement worker lifecycle, target-scoped operation records, pinned build inputs, candidate submission, and teardown without making worker availability a build/deploy prerequisite.
- [ ] Until Platform candidate orchestration is implemented, report target snapshots unavailable/stale rather than implying they are being produced.
- [ ] Integrate client snapshot verification/install under the single rolling Vapor/toolchain lock.
- [ ] Verify each target produces and restores a matching snapshot and that a stale or wrong-target snapshot is rejected.

## Task 9: Prove clean reconstruction and operator recovery

**Files:**
- Modify: Platform composed export/import and recovery scripts.
- Modify: service-owned export/import contracts only in their owning submodules.
- Modify: Vapor Client bootstrap/recheckout behavior only in its owner repository.

**Interfaces:**
- Input: clean source checkouts, signed Vapor releases/snapshots, and explicitly backed-up service state/secrets.
- Output: a rebuilt platform and client with operational service state restored, without relying on hidden working directories or game-file backups.

- [ ] Run composed export and inspect that service state is included while secrets are separately protected and game bytes are excluded.
- [ ] Reconstruct Platform from tracked root/submodule revisions and restore a test export into a clean instance.
- [ ] Reconstruct the client/source checkout and rehydrate matching target snapshots from the Build Cache service.
- [ ] Verify service health, admin readiness, product policy, and one non-destructive local-game discovery after restore.
- [ ] Document the single supported pre-alpha recovery path and remove obsolete setup instructions only after successful reconstruction.

## Program Exit Criteria

- Vapor installs and updates independently; Steam owns only the intended Loo Cast depots and optional handoff.
- Loo Cast’s first-party products remain local/depot-provided and cannot be published as community content.
- Mad Max content is locally discovered and managed without Vapor distributing game bytes.
- Identity/Registry enforce source and publication policy regardless of client claims.
- Operations work through a truthful, audited shared toolbench; privileged access is temporary, bounded, and actually verified.
- Both supported build targets have current VPS-produced signed snapshots, or a documented VPS-only resource/licensing blocker.
- A clean reconstruction succeeds from tracked source plus owned state without hidden checkout state or licensed game files.

## Execution Order and Handoff

The **Immediate First Sprint** starts the public download/getting-started lane and
admin toolbench lane in parallel. In the same period, recover the client
ownership map without merging its dirty checkouts. After the first sprint,
continue the dependency sequence: Tasks 1–3 establish verified operator access,
ownership, and canonical Vapor identity; Tasks 4–7 advance policy, install,
product, and admin work subject to those contracts; Task 8 follows a successful
VPS resource/licensing preflight; Task 9 is the reconstruction/release gate.
Detailed plans remain the code-level source of truth for each task. Before a
task spans repositories, use an isolated execution worktree and record its base
revision; never stage unrelated dirty files as part of another lane.
