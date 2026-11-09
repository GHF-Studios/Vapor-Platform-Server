# Standalone Vapor Product and Platform Design

**Status:** Approved by owner  
**Date:** 2026-11-03

> **Build snapshot update (2026-11-07):** The snapshot/worker topology in this
> earlier design is superseded by
> [`../vapor-build-snapshot-contract.md`](../vapor-build-snapshot-contract.md)
> and the owner-approved
> [`2026-11-06-vapor-platform-operations-and-recovery-redesign.md`](2026-11-06-vapor-platform-operations-and-recovery-redesign.md).
> A second build worker is optional; local source builds, service deployment,
> and primary-host recovery must remain available without it.

> **Steam packaging update (2026-11-09):** The previous optional loose
> locator and separate launch-target assumptions are superseded by the
> owner-approved [`2026-11-08-vapor-platform-and-product-rebuild-charter.md`](2026-11-08-vapor-platform-and-product-rebuild-charter.md).
> Steam ships one self-contained executable artifact embedding
> `App.vapor.toml`, Spacetime Engine, Loo Cast Game, and Loo Cast Packagepack.

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
- Vapor's local install root is the user's Vapor home (`%USERPROFILE%\Vapor`
  on Windows and `$HOME/Vapor` on Linux). Vapor owns its application, tools,
  sources, caches, configuration, and state there; Steam does not own those
  files or their lifecycle.
- The official Loo Cast Steam depot contains one self-contained executable
  artifact embedding `App.vapor.toml`, Spacetime Engine, Loo Cast Game, and
  Loo Cast Packagepack. These first-party products are depot-provided and
  non-publishable as community content.
- No separate locator/forwarder or product folders are placed in the Steam
  depot. The exact Steam-start behavior of the executable artifact remains to
  be designed; it never bundles or installs the independently managed Vapor
  runtime.
- The only local Vapor setup roles are **Player** and **Developer**. Developer
  always installs the development toolset, including both supported targets
  and managed cross-compilers; there is no skip option. Server-side ecosystem
  developer authorization remains a separate permission, not a third local
  role.
- Mad Max is a user-owned external game installation. Vapor detects the local
  installation, indexes/mounts supported source data into a separate cache,
  and manages mods against that installation. Vapor servers never distribute
  Mad Max game files.
- The Mad Max adapter uses Vapor's content layers deliberately: the locally
  acquired `MadMax.exe` and required original runtime files form the **Engine**
  source; game scripts and content form the **Game** layer; and
  `MadMaxModding` is the integration **Packagepack** that makes the installation
  receptive to Vapor-managed content. It owns the setup/repair lifecycle for
  the ASI loader and the Mad Max bridge that connects the running game to Vapor.
  The Packagepack must not contain Mad Max game files. If a third-party loader
  binary cannot be redistributed, its installer must acquire it from its
  authorized source rather than bundling it.
- Mad Max mods may target the Engine, Game, or another mod/Packagepack, with
  dependency, conflict, and override relationships represented through Vapor's
  canonical content semantics. This design establishes the layer mapping; it
  does not create a parallel manifest or identity schema before Vapor issues
  #1/#2 define those contracts.
- A content manifest cannot grant itself publish rights. Trusted product and
  source policy, enforced by Vapor Identity/Registry authority, decides whether
  content is local-only, publishable, or gated.
- One VPS is the platform control plane. Independently running service
  processes sit behind the shared Platform gateway. Platform preserves
  root-level service submodules, independent service build/deploy boundaries,
  service-owned state/policy, and composed export/import. This later topology
  supersedes the earlier monolithic-runtime assumption.
- The admin toolbench is one coherent operator experience with a shared shell
  and service-owned sections. Local game discovery, mounting, launching, and
  asset workbenches belong in the Vapor Client, not in the VPS dashboard.
- The build cache retains one current snapshot per supported target and rolling
  Vapor/toolchain lock. VPS-managed isolated Linux and Windows guest builds
  produce those snapshots; the exact guest images and virtualization mechanism
  remain implementation decisions.
- The first delivery sprint runs two independent lanes in parallel: the public
  Vapor download/getting-started journey, and the VPS operator admin toolbench.
  Their data/authority responsibilities stay separate.

## Product and interface map

```text
Vapor public site / release feed
  └─ independent Vapor installer and signed self-update
       └─ Vapor Client: launcher, Studio, source selection, product adapters,
          local cache/mounts, content profiles, and game launch
            ├─ Loo Cast product adapter
            │    └─ local Steam depot source: Engine + Game + Packagepack
            └─ Mad Max product adapter
                 ├─ local Engine source: MadMax.exe + required original files
                 ├─ Game layer: scripts/content and installed mod composition
                 ├─ MadMaxModding Packagepack: ASI loader + Vapor bridge setup
                 └─ separate derived cache; no game bytes uploaded

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

## Repository baseline and material drift

The following checkout and service observations were recorded on 2026-11-03.
They are a recovery snapshot, not assertions about live state; re-check each
repository and the VPS before acting on it.

- Platform root `45e875acd4c825c30dfe2ab8423a2527dbcc3eb0` pins Identity
  `c92ea9e0206dface4a18563d9feb428a0c66c74d`; the same-origin authorization
  fix was published at that snapshot. The SSH requester flow and local typed
  operator path have since been worked on and the typed path was owner-approved;
  this design does not assert that an SSH grant is currently active. Verify the
  actual request, grant, expiry, and audit record through the owning services.
- At the recorded baseline, the admin-toolbench design already called for a
  server-rendered shared shell, real unavailable states, task-oriented service
  workbenches, and useful operation errors. At that baseline, `src/admin.rs`
  duplicated navigation between Rust and injected JavaScript, marked every
  module available, stacked unrelated Operations tasks together, and hid
  mutation response details.
- At the recorded snapshot, Vapor Client and Loo Cast metadata still reflected
  the previous Steam/Workshop model. The accepted target is the standalone
  Vapor lifecycle and the depot-mounted Loo Cast artifact described above;
  current source and Steam metadata must be rechecked before migration.
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

### 0. Access and repository recovery

Reconcile the current Platform access request and grant state through the
service-owned approval/audit record. The typed root-only operator path is an
approved bootstrap interface, while the normal browser flow remains supported.
Neither a submitted request nor an approval screen alone proves that access is
active. Confirm key, capabilities, account, host, expiry, and audit outcome.

### 1. Reconcile authorship and active blockers

Preserve all dirty checkouts. Reconcile overlapping ownership claims for
`Vapor/**`, `Vapor-Installer/**`, and the updater/Mad Max workstreams. Establish
which checkout is authoritative for each uncommitted change; do not merge the
two client clones or reset either. Advance Vapor #1/#2 as the canonical
identity dependency; update Client and Mad Max plans to consume them.

### 2. Complete independent Vapor lifecycle

Finish signed release extraction, staging, root swap, rollback, GUI update
state, trust-key provisioning, and independent uninstall. Store source
checkouts, managed tools, cache, configuration, and state under the agreed
user-owned Vapor home. Make Steam entrypoint discovery optional and external to
Vapor's update authority. Developer setup installs all required development
tools by default; Player setup does not install them.

### 3. Establish trusted product topology

Move the Loo Cast app/depot identity to the Loo Cast product while retaining
one self-contained executable artifact with the manifest and all three
first-party products embedded. No separate locator/forwarder is shipped.
Design the executable's Steam-start behavior without bundling the standalone
Vapor runtime. Change the products' source/visibility so they cannot be
community-published. Add a
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
- Both target builds retain a local source-build path; when an optional worker
  is provisioned, it can produce candidates for service validation and
  promotion. A clean export/reinstall/import/recheckout flow restores the
  platform without distributing licensed game content.

## Decisions left open

- Exact canonical source/product identities and versioned serialization follow
  active Vapor #1/#2; this design intentionally does not mint a second schema.
- Exact Steam AppID/depot IDs, Steam-start behavior within the single
  executable artifact, Workshop retirement steps, and migration order require
  owner-controlled release metadata and are not inferred here.
- The client-owned location/layout for Vapor home subdirectories should reuse
  the current app updater and source migration contracts rather than invent a
  second path authority.
- The VPS virtualization provider/images, Windows license/source, and resource
  budget need a read-only capability preflight before builder implementation.
- The exact server-side identity representation for ecosystem-developer access
  follows Vapor Identity's service-owned contract; it must not introduce a
  third local install role.
- The SSH request/grant path's current live status must be verified from the
  owning service records; the approved typed operator interface does not replace
  the web approval path. Its contract is in
  `2026-11-03-typed-vps-ssh-operations-design.md`.
