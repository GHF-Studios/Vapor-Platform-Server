# Standalone Vapor Product and Platform Design

**Status:** Approved by owner  
**Date:** 2026-11-03

## Intent

Reconcile Vapor Client, Vapor's server platform, Loo Cast, and Mad Max into one
coherent pre-alpha product model. Vapor is an independently installed and
updated runtime/content manager. Loo Cast and Mad Max are distinct products
managed by Vapor, but their licensed game files remain locally acquired and
are never distributed by Vapor infrastructure.

This design is the cross-repository north star and migration sequence. It does
not replace service-owned implementation details or approve destructive
checkout cleanup.

## Owner decisions already established

- Vapor is installed, updated, and uninstalled independently through its own
  installer and release feed; Steam is not Vapor's distribution authority.
- The official Loo Cast Steam app owns three local first-party products:
  Spacetime Engine, Loo Cast Game, and Loo Cast Packagepack. Those products are
  depot-provided and non-publishable as community content.
- Loo Cast may keep its Steam launch choices. One starts Loo Cast; another
  opens Vapor's tools selector when Vapor is installed. If Vapor is absent, the
  entrypoint gives clear install guidance rather than installing Vapor through
  Steam.
- Mad Max is a user-owned external game installation. Vapor detects the local
  installation, indexes/mounts supported source data into a separate cache,
  and manages mods against that installation. Vapor servers never distribute
  Mad Max game files.
- A content manifest cannot grant itself publish rights. Trusted product and
  source policy, enforced by Vapor Identity/Registry authority, decides whether
  content is local-only, publishable, or gated.
- One VPS is the platform control plane. The Platform host may compose
  service-owned modules in the agreed monolithic runtime while preserving
  root-level service submodules, independent service build/deploy boundaries,
  service-owned state/policy, and composed export/import.
- The admin toolbench is one coherent operator experience with a shared shell
  and service-owned sections. Local game discovery, mounting, launching, and
  asset workbenches belong in the Vapor Client, not in the VPS dashboard.
- The build cache retains one current snapshot per supported target and rolling
  Vapor/toolchain lock. VPS-managed isolated Linux and Windows guest builds
  produce those snapshots; the exact guest images and virtualization mechanism
  remain implementation decisions.

## Product and interface map

```text
Vapor public site / release feed
  └─ independent Vapor installer and signed self-update
       └─ Vapor Client: launcher, Studio, source selection, product adapters,
          local cache/mounts, content profiles, and game launch
            ├─ Loo Cast product adapter
            │    └─ local Steam depot source: Engine + Game + Packagepack
            └─ Mad Max product adapter
                 └─ local user-owned game source + separate derived cache

Vapor VPS control plane
  ├─ Platform: composition, deployment, recovery, shared admin shell
  ├─ Identity: identity, roles, product/source authorization, audit
  ├─ Registry: catalog and publication policy/records
  ├─ Artifact: signed Vapor application releases
  └─ Build Cache: signed rolling target snapshots
```

The public download page, local Vapor Client, and VPS admin toolbench are
separate surfaces with different authority. They share product identities and
release policy; they do not become one giant dashboard or one service owner.

## Current truth and material drift

- Platform root `45e875acd4c825c30dfe2ab8423a2527dbcc3eb0` pins Identity
  `c92ea9e0206dface4a18563d9feb428a0c66c74d`; the same-origin authorization
  fix is published. Platform CI passed for the pin update. The new CLI request
  was submitted, but the SSH grant has not been confirmed and the Operations
  page did not provide a reliable way to inspect/approve it. A local typed-root
  grant path remains a separate unapproved proposal.
- The approved admin-toolbench design already calls for a server-rendered
  shared shell, real unavailable states, task-oriented service workbenches,
  and useful operation errors. Current `src/admin.rs` duplicates navigation
  between Rust and injected JavaScript, marks every module available, stacks
  unrelated Operations tasks together, and hides mutation response details.
- Current `Vapor-Client` source still treats AppID `2122620` as the Vapor Steam
  app and treats Loo Cast products as public Workshop content. The Steam
  entrypoint derives Vapor's root from its location under that app. That
  directly contradicts the approved standalone-Vapor direction.
- The current Loo Cast manifests name Steam AppID `2122620`, public visibility,
  and existing Workshop IDs for Engine, Game, and Packagepack. The target must
  move those products to the Loo Cast Steam depot and mark them non-publishable;
  no replacement AppID or migration mechanics are assumed here.
- Vapor Client's main checkout (`a5c302a`) contains distinct uncommitted
  standalone app updater work. `Vapor-Client-madmax` (`f8f91df`) contains a
  separate feature branch and dirty Vapor/CLI changes, with overlapping
  Installer/Core path claims and a workstream that explicitly excludes Loo
  Cast. The Mad Max repo (`cf698ec`) contains extensive dirty implementation,
  design, and planning work. Preserve all three states; none is safe to
  overwrite or clean up as part of this design.
- Vapor issues #1 and #2 are ACTIVE canonical Context/Subject/Selection and
  source-identity work. Vapor-Client #1–#3 are BLOCKED on those contracts.
  Product/source identity must consume those semantics instead of inventing a
  parallel game-install identity or bootstrap format.
- The standalone updater and external-game source manager are different
  lifecycles: the updater verifies Vapor releases; a game adapter detects local
  licensed files and materializes derived cache state. They must share stable
  identity and recovery semantics without conflating source bytes or authority.
- The Build Cache service and contract exist, but VPS-managed builder
  orchestration and client snapshot restore/bootstrap are not yet proven.

## Approaches

1. **Keep Steam as Vapor's primary installation and make games ordinary
   content.** Smallest change to current source, but it directly violates the
   standalone installer requirement and incorrectly puts licensed first-party
   game products in the publishable content model.
2. **Create separate Vapor installs per game.** This avoids product adapters,
   but duplicates toolchains, caches, identity, update state, and operator
   workflows.
3. **One standalone Vapor runtime with product-specific source adapters and
   authoritative product policy (selected).** Vapor owns one install/update
   lifecycle; Loo Cast and Mad Max remain distinct products with different
   local acquisition adapters and publication rights. This matches prior owner
   decisions and keeps the shared runtime reusable.

## Ownership boundaries

| Concern | Owner | Boundary |
| --- | --- | --- |
| Canonical source/product identity and Context/Subject/Selection | Vapor core | Complete active #1/#2 contracts; no game-specific duplicate IDs |
| Vapor install, signed release verification, rollback, launcher and local product adapters | Vapor Client | Independent user-home installation; no Steam app-root ownership |
| Loo Cast source products and build/depot composition | Loo Cast + Vapor Client adapter | Steam depot is the trusted local source; three first-party products are local-only and not community-publishable |
| Mad Max game runtime/mod package | MM-Building-System | Game remains user-owned; client adapter discovers/mounts it; runtime never uploads game bytes |
| Product access and publish authority | Vapor Identity | Server-side policy; client manifests and UI flags cannot override authority. Identity permission changes stay in Identity-owned requests. |
| Catalog records and community package visibility | Vapor Registry | Owns catalog state and publication eligibility records; not Steam metadata |
| Vapor app release feed | Vapor Artifact | Signed immutable Vapor app releases; no game payloads |
| Rolling target snapshots | Vapor Build Cache | One current signed snapshot per supported target/lock; promotion is service-owned |
| VPS composition, hosting operations, state export/import, shared operator shell | Vapor Platform | Does not absorb service business logic or local game workflows. Platform deploy approval remains bound to the exact root revision and pinned submodule revisions. |

## Delivery sequence

### 0. Access bootstrap

Recover a working, explicitly authorized Platform operator path for the current
machine. The submitted CLI request still needs an actual Platform grant. The
candidate paths are to fix and use the existing root-session Operations flow,
or to approve and separately implement the narrow local typed-root grant
proposal. Until one is completed, the grant is not active. A local root command
is not a permanent key allowlist and is not the dashboard architecture. Confirm
key, capabilities, account, host, and expiry before treating access as active.

### 1. Reconcile authorship and active blockers

Preserve all dirty checkouts. Reconcile overlapping ownership claims for
`Vapor/**`, `Vapor-Installer/**`, and the updater/Mad Max workstreams. Establish
which checkout is authoritative for each uncommitted change; do not merge the
two client clones or reset either. Advance Vapor #1/#2 as the canonical
identity dependency; update Client and Mad Max plans to consume them.

### 2. Complete independent Vapor lifecycle

Finish signed release extraction, staging, root swap, rollback, GUI update
state, trust-key provisioning, and independent uninstall. Store source
checkouts, managed tools, cache, and state under a coherent Vapor home. Make
Steam entrypoint discovery optional and external to Vapor's update authority.

### 3. Establish trusted product topology

Move the Loo Cast app/depot identity to the Loo Cast product while retaining
its three depot-local artifacts and Vapor tools-selector launch option. Change
their product source/visibility so they cannot be community-published. Add a
Mad Max external-game source adapter that detects the user-owned install,
maintains a separate derived asset cache, and uses reversible mod receipts.
Do not put game archives, textures, executables, or other licensed bytes in
Artifact, Registry uploads, Build Cache, or public diagnostics.

### 4. Bring Registry/Identity/Artifact semantics online

Enforce trusted product-source and publishability decisions server-side;
publish only mod/community data for which the authority grants rights. Keep
Vapor executable releases distinct from game-content catalog items and target
build snapshots. UI visibility is presentation, never authority.

Preserve the existing approval contracts: Platform deployment approval names
the exact tested root revision and submodule pins; Identity role/permission
changes remain Identity-owned and audited; request-only automation credentials
can submit requests but cannot approve them or execute privileged operations.

### 5. Build the operator toolbench around real operations

Replace the current duplicated shell with a real shared frame and truthful
implemented/unavailable states. Organize service-owned sections around Overview,
Catalog, Identity, Releases, Build Cache, and Operations (Deployments, Hosting,
Temporary Access, History). Every state-changing action shows its requester,
exact target, policy, confirmation, operation status, and audit/result. Keep
local game mount/profile workflows in Vapor Client.

### 6. Finish target snapshots and reconstruction

Prove VPS capacity/licensing/virtualization for both pinned target guests,
then implement the shared isolated VM lifecycle and per-target build/publication.
Complete export/import and clean source reconstruction so a pre-alpha server
can be rebuilt from source plus owned state without preserving ad hoc working
directories. Keep secrets outside ordinary state exports and game files outside
Vapor distribution.

## Acceptance

- Vapor installs, updates, repairs, and uninstalls without Steam, and its
  signed feed never contains Loo Cast or Mad Max game files.
- Steam owns the Loo Cast first-party depots and provides a thin optional
  handoff to Vapor; it does not install Vapor or own Vapor's user data.
- Vapor detects and manages a local Mad Max install with a separate derived
  cache and reversible mod lifecycle; no game bytes are published or uploaded.
- Server authority rejects attempts to publish depot-only or externally
  licensed products even when a client manifest claims otherwise.
- The public download site, local client, and web admin have distinct goals and
  ownership, while using the same product identity and policy contracts.
- The VPS dashboard shows truthful service ownership/readiness and can
  complete/audit its real admin workflows; it never claims placeholder pages
  are usable.
- The dashboard presents the existing exact-revision deploy and
  service-owned permission-approval workflows; a confirmation checkbox alone
  never substitutes for the owning service's authorization decision.
- Both target build snapshots are reproducible by VPS-managed isolated guests,
  and a clean export/reinstall/import/recheckout flow restores the platform
  without distributing licensed game content.

## Decisions left to implementation planning

- Exact canonical source/product identities and versioned serialization follow
  active Vapor #1/#2; this design intentionally does not mint a second schema.
- Exact Steam AppID/depot IDs, Workshop retirement steps, and migration order
  require owner-controlled release metadata and are not inferred here.
- The client-owned location/layout for Vapor home subdirectories should reuse
  the current app updater and source migration contracts rather than invent a
  second path authority.
- The VPS virtualization provider/images, Windows license/source, and resource
  budget need a read-only capability preflight before builder implementation.
- The owner authorized a typed local-operator SSH path before approving this
  cross-product design. Its exact scope, confirmation, audit, and lifecycle
  contract is in `2026-11-03-typed-vps-ssh-operations-design.md`; it remains a
  bootstrap interface and does not replace the web approval path.
