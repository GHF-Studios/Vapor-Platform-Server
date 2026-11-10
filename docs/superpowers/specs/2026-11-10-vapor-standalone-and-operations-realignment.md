# Vapor Standalone and Operations Realignment

**Status:** Owner-authorized implementation direction
**Date:** 2026-11-10

## Intent

Make Vapor a standalone runtime and operator workbench whose lifecycle is
independent of the Loo Cast Steam app. Keep a compact public entry point,
clear product-specific destinations, and one authenticated operations
workspace. Make the server recoverable from source plus owner-held state, and
make the optional second VPS useful for deliberate replacement/recovery drills
without introducing autoscaling or guessed server roles.

## Owner decisions

- Vapor is installed, updated, repaired, and fully uninstalled independently
  from Steam. Vapor owns its binaries, toolchains, source checkouts, build and
  download caches, content, configuration, installation records, and mutable
  state under its canonical user home and the documented per-user data roots.
- Steam is a product/content mount. The Loo Cast Steam depot carries only the
  Loo Cast Packagepack artifact and `Packagepack.vapor.toml`; platform depots
  carry one primary Linux or Windows Packagepack executable. Required DLLs or
  other runtime sidecars may accompany it where technically necessary; “one
  executable” does not mean “one physical file”. Do not ship `App.vapor.toml`,
  Vapor runtime files, tools, sources, checkout data, separate Engine/Game
  product directories, or a Vapor locator in the depot.
- Mad Max remains a separately acquired Steam game. Its integration layer is a
  Vapor Packagepack and contains no Mad Max game files.
- The local role set is exactly Player and Developer. Developer means all
  required development tools are installed; there is no skip-tools state.
- The platform has separate application service processes behind one public
  gateway. A second host is a manually operated replacement/recovery target,
  not an autoscaling worker role.
- The server UI is the graphical interface to service-owned records and typed
  operations, not a second authority. A browser terminal is an explicitly
  separate emergency capability, not the normal operations model.
- “Full nuke” is a distinct, explicit uninstall action. It removes Vapor-owned
  active, cached, authored, and legacy data from inventoried Vapor locations.
  It must show a path-level preview and must never silently delete unrelated
  game files, other applications, arbitrary similarly named directories, or
  user data outside those known locations.
- Build, test, local install, commit, push, and deployment are in scope when
  their owning workflows are ready. Preserve all unrelated dirty Git state.

## Product-specific interface direction

The user supplied `heydamir/unslop` and asked that its method be applied. The
skill is CC BY 4.0 and requires attribution for copied/adapted material. Vapor
will use its process, not copy its example design.

### Shared design direction

- **Product:** Vapor installs, composes, and operates versioned application
  content and its host services.
- **Character:** exact, workshop-like, quietly technical.
- **World:** instruments, manifests, mounted content, and service records.
- **Moments:** real deployment stages and host telemetry; actual Packagepack
  identity and target.
- **Signature:** a compact source/target marker showing which system owns an
  artifact or action.

Keep existing recognizable Vapor identity where it is real. Use compact,
aligned rows, restrained neutral surfaces, one meaningful accent, sentence
case, consistent numeric formatting, and purposeful density. Keep real product
features and workflow order. Do not invent claims or show mock data as live.

## Goals

- Make the public homepage explain what Vapor is, how to install it, and how
  Loo Cast and Mad Max relate to the standalone runtime.
- Make sign-in, account, public product workspaces, and private admin access
  visibly connected while preserving service-owned authentication and records.
- Make Approvals a review queue, Deployments a deployment lifecycle, SSH
  Access an access lifecycle, Servers a host operations workbench, History an
  event record, and Recovery a whole-system export/import and restore surface.
- Compact the admin interface without losing decision-critical details,
  stage ordering, freshness, target, owner, or outcome.
- Apply the same product-specific UI review to Vapor Studio/Installer instead
  of treating the web and desktop experiences as unrelated products.
- Move all Vapor-owned files out of the Loo Cast Steam root into canonical
  Vapor-managed paths, with a transaction and a user-visible recovery result.
- Provide an ordinary uninstall and a separate full-nuke uninstall, both
  independent of Steam.
- Package one primary Packagepack executable per supported Steam target with
  `Packagepack.vapor.toml` as the common manifest. Permit only explicitly
  required runtime sidecars in addition to that executable.
- Keep independent service builds/deploys and preserve composed whole-system
  export/import.

## Non-goals

- Autoscaling, provider account automation, automatic server role assignment,
  or automatic migration to a second VPS.
- Making a public browser shell the normal way to operate the host.
- Sending licensed Loo Cast or Mad Max game data through Vapor Registry,
  Artifact, or other Vapor-hosted distribution.
- Retaining obsolete Steam-root layouts, old local role models, or
  compatibility branches in the pre-alpha product.
- Generating visual marketing art for dense admin or desktop screens when
  actual product data and interface components are the right visual material.

## Current evidence

- The main Platform checkout already contains a large uncommitted host-first
  and service-separation pass. It includes local `/proc` process telemetry but
  the approved design still calls for remote-host parity. Do not overwrite the
  dirty changes or claim the feature is complete before review and validation.
- The active Vapor Client checkout is deeply dirty across root files and
  submodules. It already contains standalone-home, role, updater, and UI work.
  Its current `AGENTS.md` and foreground workstream reflect the agreed Steam
  contract: `Packagepack.vapor.toml`, one primary Packagepack executable per
  OS depot, and only runtime sidecars that executable actually requires. The
  executable composes Engine and Game; Steam does not receive separate
  Engine/Game product payloads, `App.vapor.toml`, or Vapor itself.
- The current Loo Cast `App-Source.vapor.toml` stages the Packagepack manifest
  and one target-specific executable for each OS. The Packagepack manifest
  references its Engine and Game by content identity, and its Cargo target
  links those products into the executable. Sidecar DLLs are permitted if
  dependency inspection proves they are required; they do not create another
  product entry point. The old three-directory Steam layout is no longer the
  target contract.
- The current Platform admin palette is already restrained and contains no
  dominant gradients or emoji navigation. Its remaining visible issues are
  inconsistent density/rhythm, uppercase tracked labels, repeated verbose
  helper text, and component-specific spacing/radius choices.
- `C:\Users\David Jackson\Vapor`, AppData Local/Roaming Vapor, and the Loo
  Cast Steam root contain overlapping Vapor files. A read-only scan found the
  Steam root's generated `output` directory at roughly 26.8 GiB, content at
  3.99 GiB, and toolchain directories at about 4.5 GiB combined. These sizes do
  not establish that a Steam-root copy is redundant; compare file ownership and
  hashes before moving or deleting.

## Ownership boundaries

| Work | Owner |
| --- | --- |
| Host operations, deployment orchestration, composed recovery | Vapor Platform Server |
| Homepage, product pages, public downloads and legal content | Homepage service |
| Sign-in and account records | Identity service |
| Admin shell and service-owned views | Owning service; shared gateway only routes/authenticates |
| Vapor home, installer, updater, full-nuke workflow, generic desktop UI | Vapor Client and its owned submodules |
| Loo Cast Engine/Game/Packagepack manifests and Steam package | Loo Cast |
| Mad Max game adapter and scripts | MM-Building-System |

Do not move service business logic into Platform. Keep source changes in the
owning repository; coordination documents in this Platform checkout do not
change service ownership.

## Approaches considered

1. **Continue small CSS-only repairs and leave Steam packaging as a manifest
   rename.** Rejected: it would not fix the confusing UI ownership or remove
   Steam's current multi-product payload, and would confuse “one executable”
   with “one physical file” despite legitimate runtime dependencies.
2. **Replace all existing screens and workflows with a new generic dashboard.**
   Rejected: it would erase useful owner/service semantics and make the
   interface a second authority.
3. **Selected: correct ownership and artifact boundaries first, then apply a
   compact, product-specific design pass over existing service and desktop
   surfaces.** This preserves working service APIs while allowing real
   structural redesign where the current ownership or package boundary is
   wrong.

## Implementation calls

- Correct stale design docs and source instructions rather than retaining two
  contradictory “approved” targets.
- Keep the shared admin palette's dark graphite/ink character and mint action
  accent; fix spacing, labels, information hierarchy, focus, and responsive
  layout before considering additional colors or imagery.
- Use actual API data only. Missing services, unready products, and unavailable
  metrics must appear as unavailable, never as placeholder values.
- The full-nuke workflow uses a preflight inventory generated from the
  installation/path owners, a reviewed path list, explicit confirmation,
  staged cleanup, and a durable completion/error report. It does not recurse
  through a whole drive looking for names containing “Vapor”.
- Keep archived/exported platform state separate from host configuration.
  Whole-system restore composes independently owned service bundles; rebuilding
  units and gateway configuration is a reproducible setup operation.

## Acceptance criteria

- Documentation across Client and Loo Cast states one current Steam payload
  contract: common `Packagepack.vapor.toml`, one target-specific primary
  Packagepack executable, and only required runtime sidecars; no
  `App.vapor.toml` or separate Engine/Game product directories.
- A generated Steam staging manifest contains the declared Packagepack
  manifest, one correct primary executable for the selected platform, and only
  declared runtime sidecars. It rejects undeclared files and any App manifest.
- The Packagepack target resolves its Engine/Game composition without separate
  Engine/Game product payloads in the depot.
- Vapor binaries, tools, sources, caches, settings, content, and state live in
  canonical Vapor-owned roots after migration; Steam is not a Vapor home.
- Ordinary uninstall retains user-authored state as described in its explicit
  choices; full-nuke previews every Vapor-owned canonical and known legacy path
  and removes them only after explicit confirmation.
- Sign-in and public/admin navigation have consistent paths and accurate
  state; logged-out users do not see private admin internals.
- Admin approval and deployment screens preserve correct queue ordering,
  current revision, owner, target, execution stage, and failed/retry state while
  removing redundant information and excess vertical space.
- Server host operations remain typed, recorded, and independent of service
  deploys. Replacement/recovery remains a deliberate composed operation.
- Local Vapor UI and web UI share recognizable language and useful density.
- Relevant Linux and Windows builds and focused tests pass before release; the
  installed Windows and Linux Steam experiences are verified by the owner
  before any public release.

## Attribution

This design applies the workflow and visual principles of [unslop by Damir
Sokolovsky](https://github.com/heydamir/unslop), licensed CC BY 4.0. No example
screen or code has been copied.
