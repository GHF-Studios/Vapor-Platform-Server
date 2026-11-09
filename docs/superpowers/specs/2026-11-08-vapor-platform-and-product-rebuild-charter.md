# Vapor Platform and Product Rebuild Charter

**Status:** Current owner-approved direction; implementation must follow the staged plan. This charter supersedes conflicting earlier target decisions, while historical observations remain historical.
**Date:** 2026-11-08
**Scope:** Cross-project boundaries and the Platform-owned operational/recovery layer. Product behavior remains in its owning repository.

## Mission

Make Vapor a standalone runtime and product ecosystem, not a Loo Cast Steam-app installation. Make the VPS and its web presence understandable, recoverable, and useful through task-focused interfaces. The owner must be able to rebuild the Platform from source and a local physical recovery set after replacing the VPS, while ordinary workflows stay simple and service-owned.

This is pre-alpha. Weak implementations may be replaced outright. The acceptable cost of a failed experiment is lost time, not stranded authority or irreplaceable data. Preserve useful historical evidence, but do not retain compatibility paths merely because they exist.

## Target shape

```mermaid
flowchart TB
  Steam[Steam: one self-contained Loo Cast executable artifact]
  Steam -. optional tools launch, if retained .-> Runtime[Standalone Vapor install in Vapor home]
  Runtime --> LC[Loo Cast project adapter]
  Runtime --> MM[Mad Max Modding project adapter]
  Runtime --> Sandbox[Developer sandbox]
  Runtime --> Tools[Vapor Studio / developer tools]

  Browser[Public browser] --> Public[Standalone download, getting started, docs]
  Browser --> User[Signed-in personal dashboard]
  User --> Project[Authorized project workspaces]
  Browser --> Admin[Root-authorized Platform operator toolbench]
  Admin --> Gateway[Shared admin gateway]
  Gateway --> Platform[Platform: deployment, host, recovery]
  Gateway --> Identity[Identity: accounts, roles, authorization, audit]
  Gateway --> Services[Registry / Artifact / Build Cache / Docs / Diagnostics]
  Platform --> Main[Main Strato VPS]
  Worker[Optional worker / replica] -. unavailable => local build fallback .-> Main
  USB[Owner-held removable recovery drive] --> Restore[Fresh-host import after Strato OS reinstall]
  Main --> USB
```

### Project and content boundary

- A **Project** is the top-level workspace and access scope for something
  people build or manage, such as Loo Cast, Mad Max Modding, or a new game
  built on Spacetime Engine. Project identity is distinct from a Steam AppID,
  repository, content item, release, or installable composition. Registry is
  the target owner of project metadata and content relationships; Identity
  remains the authority for accounts and project access grants. The dashboard
  presents these records but does not create a second source of truth.
- A **composition** is a named runnable setup made from exact compatible
  content releases and any required locally acquired sources. It is the
  install/launch selection inside a Project, not another project identity.
  Do not introduce a separate Product entity until an actual workflow needs a
  distinct product grouping above compositions.
- **Engine, Game, Packagepack, and Mod** are distinct content roles/types. A
  Packagepack is not inherently a Game or a Project; a Game need not be shipped
  as a Packagepack. One Packagepack archive may contain several typed content
  entries. Content can be reused by compositions in more than one Project.
- Steam AppIDs, Workshop item IDs, repository URLs, local install paths, and
  Artifact release URLs are provider/source references. They describe where a
  release is acquired, not its Vapor identity, content type, project identity,
  or publication authority.

### Standalone runtime and project boundary

- Vapor is installed, updated, and uninstalled independently of Steam. Its managed binaries, source checkouts, toolchains, caches, settings, and state live under Vapor-owned user/system locations, not in a Steam app directory.
- Vapor is one installed tool with one recognizable entrypoint for Players,
  Developers/Modders, Operators, and Ecosystem Developers. Their capabilities
  and workspaces differ, but they are reached through explicit context and
  access bridges inside the same product; a higher access tier does not require
  a separate Vapor installation or an unrelated product surface. Local
  Player/Developer setup and server-authoritative operator/ecosystem grants
  remain distinct authorities.
- The Loo Cast Steam depot contains one self-contained executable artifact.
  That artifact embeds `App.vapor.toml` and the three first-party local
  products: Spacetime Engine (Engine), Loo Cast (Game), and Loo Cast
  Packagepack (Packagepack). Do not split those products into separate depot
  folders or add a loose Vapor locator/helper. The executable artifact may
  provide the Steam launch entrypoint into the separately installed Vapor
  tools selector; it must show clear install guidance when Vapor is absent and
  must never bundle or implicitly install the Vapor runtime. Steam content
  must not carry developer toolchains, build caches, or Vapor user state.
- The three first-party Loo Cast content entries are depot-provided/local-only
  and cannot be made community-publishable by editing `App.vapor.toml` or
  another client manifest. The owning server policy is authoritative.
- Mad Max remains a separately acquired local game. Vapor Client's Mad Max
  adapter detects and mounts the user's installation and maintains derived
  data in Vapor-owned paths. It manages the separate `MadMaxModding`
  Packagepack, which carries the reversible loader/bridge integration and
  configuration; the adapter is client software, not Packagepack content. No
  proprietary game bytes are uploaded or redistributed.
- A clean-room uninstall is an explicit, inspectable operation that inventories
  Vapor-owned current and historical paths, Steam integration, registrations,
  tasks, source checkouts, caches, and unsaved data. It is not the default
  uninstall. Its preview enumerates every selected path; unknown/unclassified
  paths are preserved. A separate full-purge choice may delete inventoried
  Vapor-owned unsaved data only after export offer and exact destructive
  confirmation. It must never delete original Mad Max files or arbitrary user
  data.

### Server and web presence

- One physical VPS may run multiple independent service processes behind one public domain and a shared authenticated admin gateway. The Platform repo owns composition, fixed host operations, deployment orchestration, backup/import/recovery, and the shared shell. Each service repo owns its business rules, records, policy, and service-specific interface fragment.
- The web dashboard is the normal graphical interface into these services; “service-owned authority” does not mean the owner must use the CLI. The gateway may present and invoke workflows, but never shadows owner records or bypasses their authorization/audit.
- The target browser experience uses one recognizable site shell with clear
  bridges among public pages, a signed-in personal dashboard, authorized
  Project workspaces, and the root-only Platform operator area. The entry to
  privileged work is visible and explains its access requirement, including to
  signed-in users who lack that role. This bridge reveals no admin layout,
  private service details, operator scripts, or protected data. Admin
  authorization must succeed before the admin shell is rendered, not merely
  before protected records or actions load. Project pages require
  project-scoped authorization, and owning services recheck it for every API
  read or mutation. A route being hidden is not an authorization check.
- The browser and installed Vapor client should teach the same interaction
  model: one account/product surface, visible workspaces and contexts, and
  explicit locked or unavailable states that name the required access and
  where to obtain it. Product-specific workbenches appear inside that shell;
  they are not separate login universes. Public visitors may see the operator
  entry bridge, but never the private admin shell or its implementation.
- Platform now checks root Identity authorization before serving any `/admin`
  page, fragment, style, or script. Admin-origin isolation remains a separate
  routing defense: it does not replace authorization, and while it is
  unconfigured the admin route may still share the public origin.
- The personal dashboard is account-scoped and lists only the current user's
  profile, installations/entitlements as provided by their owners, and Projects
  they can access. A Project workspace composes owner-service views for its
  content, releases, acquisition sources, and compositions; it does not copy
  those records into a dashboard database. Product-specific UI is selected by
  Project/content context, not by Steam AppID or Packagepack type.
- Public pages serve Vapor downloads, clear product-aware getting-started flows, and working documentation. Admin tools are separate from public user flows. A narrowly scoped one-host emergency browser terminal is now in scope in a separate high-security dashboard area; it is not a general remote-shell product and remains disabled until its security prerequisites are met.
- Project workbenches are specialized views for Mad Max Modding, Loo Cast, and
  other Projects. A developer Sandbox is a personal/developer workspace unless
  it acquires a separately managed Project identity. Shared UI primitives are
  encouraged; licenses, local source detection, and content policy remain
  separate. Local game discovery, mounting, and launch stay in Vapor Client;
  server workspaces manage only server-owned catalog, publication, account, and
  release workflows.
- The emergency browser terminal is equivalent to root SSH on the one Platform VPS. It requires root Identity authorization, a separate explicit short-lived session grant, a single connection, expiry and revocation, and lifecycle audit without transcript storage. This does not replace Strato VNC or owner SSH. The current shared public/admin origin remains a blocker: a same-origin script-injection flaw can issue requests as the root operator. Keep the host switch off until the admin origin/session boundary is isolated or Identity provides a verifiable step-up signal. Other privileged work remains bounded, named, and audited; do not turn this into arbitrary multi-host SSH, port forwarding, file transfer, or an always-on shell.

### Authority and identity

- Identity owns accounts, sessions, linked providers, roles, permission requests, grants, and audit. Platform owns deploy requests and temporary host access operations. The dashboard is the front door, not a second database.
- Steam, GitHub, Strato, and Vapor each retain distinct trust boundaries. Provider integration status must be visible and revocation/relinking explicit.
- Current owner preference is Steam-required sign-in because Valve is the
  preferred identity authority. Earlier the owner also selected Vapor-native
  accounts with optional providers; treat Steam-required as the current
  direction, but hold account migration until the owner confirms the final
  model and the migration is designed. Provider linking must not silently
  create duplicate Vapor identities.
- SSH access is requested by the machine/requester, with a public key generated and retained locally; the platform stores only the public key/fingerprint and bounded grant metadata. Submission authorization and actual host grant are distinct decisions. Do not display or transmit private keys. The narrow one-host emergency browser console described above is the explicit root-terminal exception; it is not generic SSH access and must remain disabled until its security gate is satisfied.

### Recovery and worker model

- The Strato account and OS-reinstall control are the outer recovery boundary.
  For full host compromise or uncertain integrity, perform an explicit Strato
  OS reinstall (manual Strato control panel if no safe provider API exists),
  then run the pinned fresh-host recovery kit. A Platform *state reset* is a
  different named operation; it must not pretend to reinstall the OS or erase
  the host. Both paths inventory and export first. Full-host recovery requires
  reauthorizing the owner's SSH key through the Strato boundary.
- Frequent primary-host exports and the owner-held unencrypted 128 GB USB
  backup are the baseline. The backup is plaintext because the owner chose
  physical custody and portability; access to the USB is equivalent to access
  to all included service secrets. Owner SSH private keys are never included in
  VPS exports or recovery archives; the owner reauthorizes them through Strato
  after reinstall. Keep USB custody controlled and add another copy when
  practical.
- Export checksums detect corruption but not malicious replacement by a compromised VPS. A clean-host recovery drill is the evidence that matters. It must prove restore of services/configuration/data and record what remains external (DNS, Strato account, provider login, Steam/GitHub authorization).
- A second low-cost node is optional and may provide builds, staging, or redundant backup. It is never required to deploy: local builds remain a supported fallback. Do not assume Docker, nested virtualization, or specific billing before measuring the chosen node.
- Current main VPS is resource constrained; concurrent native Windows/Linux building there is not assumed. Build worker topology remains a separate measured design.

## Ownership matrix

| Concern | Owner | Explicitly not owned here |
| --- | --- | --- |
| Host provisioning, Caddy/systemd, whole-system export/import, fixed operations, deployment lifecycle | Platform Server | Identity/catalog/product business rules |
| Accounts, login, linked providers, roles, grants, access policy, identity audit | Identity Server | Deployment queue or host filesystem |
| Catalog, publication eligibility, source bindings | Registry Server | Steam app content or client-side publication claims |
| Project identity, typed content relationships, releases and compositions | Registry Server (target) | Accounts/sessions/roles, acquisition-provider authority, or dashboard copies |
| Project access and member grants | Identity Server (target) | Project/content records or service-owned business state |
| Signed Vapor application releases | Artifact Server | Loo Cast/Mad Max game assets |
| Signed Cargo target snapshots | Build Cache Server | Shared per-compile compiler cache, native-build job lifecycle, application releases, or generic user files |
| Public homepage/docs service data and rendering | Homepage/Docs service repos | Platform composition and deployment policy |
| Standalone install/update, local paths, product adapters, workbenches | Vapor Client and product adapter repos | VPS identity or host deploy authority |
| Mad Max native integration and bridge source | MM-Building-System | Vapor generic installer/update lifecycle |
| Native Loo Cast engine/game/packagepack source and depot packaging | Loo Cast repo | Vapor runtime or user's generic home |

Registry and build terminology are further constrained by
[`../../vapor-content-and-build-domain-contract.md`](../../vapor-content-and-build-domain-contract.md): source topology is not a typed content catalog, a Workshop binding is not a content identity, and a restorable Cargo target snapshot is not a shared compiler cache or native-build job.

## Explicit exclusions

- Do not distribute proprietary Loo Cast or Mad Max data through public Artifact, Registry, Build Cache, or diagnostics.
- Do not use the editable client manifest as entitlement/publish authority.
- Do not equate Project, composition, Game, Packagepack, Steam AppID, or
  provider listing. Do not add redundant Product identity until a concrete
  workflow requires a grouping distinct from Project and composition.
- Do not let one server/service own every workflow or move service business logic into the Platform root.
- Do not perform the production host wipe until a fresh-host restore drill and an owner-reviewed exact deletion inventory exist.
- Do not indiscriminately clean dirty checkouts, Ghidra databases, source caches, or game directories. Provide a reviewed clean-room operation and classify each path first.
- Do not turn the one-host emergency console into a general shell service, enable it by default, or distribute a permanent general-purpose SSH key. Browser root access stays an explicit, time-bounded emergency capability with its own security gate.

## Open decisions that block only their dependent work

1. Steam-required public Vapor account sign-in remains the owner's current
   policy. The Project workspace work adds a normal public session without
   granting global roles and keeps root operator login as a separate session;
   it does not change linked-provider or account-merge policy.
2. Exact launch behavior of the self-contained Steam executable when it is
   started from Steam, including how it finds the separately installed Vapor
   tools selector and reports when Vapor is absent. The depot remains one
   executable artifact containing its embedded manifest and three products.
3. Which Windows locations are Vapor-owned historical paths and which user-created checkouts are valuable work. Generate a dry-run inventory and allow review before clean-room deletion.
4. Second node provider/OS/capacity/cost and whether it is a builder, backup replica, or both. Keep local fallback until measured.

## Acceptance evidence

- A clean Windows and Linux install starts Vapor without the Loo Cast app
  installed. Steam's optional launch action locates Vapor without owning its
  runtime, and reports installation guidance when Vapor is absent.
- Steam depot contains one self-contained executable artifact embedding
  `App.vapor.toml` and the Engine, Game, and Packagepack products; it contains
  no separately loose locator, Vapor-managed runtime, toolchains, state, or
  caches.
- Vapor uninstall and full clean-room purge report exact paths and have separately tested preserve/export and delete modes.
- Public download/getting-started/docs links work; Platform admin pages are task-oriented, compact, accessible, and have real unavailable/failed/partial states.
- Public, personal dashboard, Project workspace, and operator experiences have
  distinct route/layout boundaries. Unauthenticated visitors cannot retrieve
  admin chrome; signed-in non-operators cannot retrieve it either. Project
  access is checked by Identity and by the owning service API.
- A browser cannot obtain arbitrary shell access. Privileged operations are typed, owner-gated, audited, and attributable.
- The emergency console stays inaccessible unless its host gate is explicitly enabled, root authorization succeeds, and the separate session grant is active. A clean security review records the admin-origin/step-up prerequisite before anyone enables it.
- Recovery on a disposable fresh Ubuntu host verifies service-owned state,
  Caddy routes, login/auth flows, exact source/submodule pins, and a repeat
  export. The drill must not use the production host. External provider
  prerequisites and owner SSH reauthorization are documented.
- Service deploy remains independently testable; full-system export/import composes pinned service revisions.
- Worker absence does not prevent a locally built release/deployment.
