# Vapor Standalone and Operations Realignment Plan

> **Execution:** Native implementation in owning repositories, preserving all existing dirty work. This is the approved direction; no new design approval checkpoint is required.

**Goal:** Finish the platform/desktop/web realignment and ship Steam as a
Packagepack mount, while keeping service authority and recoverability explicit.

**Spec:** `docs/superpowers/specs/2026-11-10-vapor-standalone-and-operations-realignment.md`

## Global constraints

- Vapor home, not Steam, owns Vapor binaries, toolchains, source, caches,
  content, config, and mutable state.
- Steam carries only the Loo Cast Packagepack manifest and one primary
  target-specific Packagepack executable. Required DLLs or runtime sidecars
  may accompany it when needed.
- Platform owns orchestration; submodules own service records and business
  logic. Preserve per-service deploy and composed system recovery.
- Full-nuke only targets inventoried Vapor-owned paths and requires an explicit
  path-level preview and confirmation.
- Preserve unrelated changes, index state, untracked files, and authored work.
- Do not publish licensed game data through Vapor services.

## Review focus

- The Steam allowlist must reject `App.vapor.toml`, undeclared files, and an
  executable for the wrong target; only declared, necessary sidecars are valid.
- Packagepack must resolve its Engine and Game at runtime without separate
  Engine/Game product payloads; explicitly required runtime sidecars are okay.
- Steam-root migration must preserve both sides on conflict or interruption.
- Full-nuke must not follow symlinks/junctions outside registered Vapor roots
  or erase similarly named unrelated directories.
- Approval UI must disable out-of-order, stale, expired, malformed, or
  superseded deploys and display the exact current stage/outcome.
- Host process/network data must remain bounded and redact process arguments,
  environments, payloads, and peer details as designed.
- Cross-platform setup/uninstall must choose the correct target paths and must
  never delete the active installation before a verified replacement exists.
- Dashboard/API navigation must not confuse authentication, authorization,
  approval, queued execution, and completed deployment.

## Phase 1: Reconcile source truth and close host operations

**Owners:** Platform Server, with service-owned changes only in submodules.

- [ ] Update the existing standalone design, plan, Client `AGENTS.md`, and
  Loo Cast source docs to supersede the old App manifest + three-product
  depot contract.
- [ ] Inspect the complete dirty Platform diff and preserve pre-existing edits.
- [ ] Finish remote-host process snapshot parity if the existing agent contract
  can support it without adding a service-owner leak.
- [ ] Remove stale internal host-role data through the documented scoped reset;
  preserve Identity, deploy, SSH, and operation records.
- [ ] Review install/recovery scripts for process separation and deterministic
  reinstallation from source plus state bundle.
- [ ] Run focused Rust, Python, shell, and repo CI checks; fix failures in the
  same coherent pass.

## Phase 2: Build the actual Packagepack Steam artifact

**Owners:** Loo Cast for manifests/composition; Vapor Client for packaging
providers and SteamPipe staging.

- [ ] Ensure the Packagepack executable resolves its Engine/Game composition.
  Ship one primary executable per target and only declared runtime sidecars
  when required; do not ship separate Engine/Game product payloads.
- [ ] Review the current root `App-Source.vapor.toml` as the Steam allowlist:
  it must stage `Packagepack.vapor.toml` in the common depot and one primary
  Packagepack executable per OS target, with only runtime sidecars that
  dependency inspection proves necessary. Reject `App.vapor.toml`, separate
  Engine/Game directories, and undeclared files.
- [ ] Stage `Packagepack.vapor.toml` in the shared depot and one primary Linux
  or Windows Packagepack executable in the matching OS depot, plus only
  explicitly declared runtime sidecars when needed.
- [ ] Update CLI schema validation, target selection, fingerprinting, preview,
  and docs to make any undeclared file fail staging.
- [ ] Build both release targets and inspect staged file inventories and
  dependencies before any Steam upload.

## Phase 3: Complete standalone home migration and full-nuke uninstall

**Owner:** Vapor Client and its owned Core/Installer/CLI submodules.

- [ ] Canonicalize all home, data, cache, tools, source, and config roots across
  Windows and Linux. Keep platform/ABI-specific artifacts target-scoped.
- [ ] Reconcile source checkouts and install ledger with every existing dirty
  checkout before moving or deleting Steam-root data.
- [ ] Implement an idempotent Steam-root move with conflict preview, atomic
  staging, rollback, and an operation record.
- [ ] Implement normal uninstall and a separate “Remove all Vapor data” mode.
  Inventory registered current and historical Vapor roots; show the exact
  affected paths and sizes; require explicit confirmation; never infer a path
  from a substring or follow directory links.
- [ ] Remove retired Steam-root fallback discovery, role names, obsolete
  locators, and compatibility paths after current installations migrate.
- [ ] Verify migration and both uninstall modes with isolated temp fixtures
  before touching the live user installation.

## Phase 4: Redesign the web presence around service-owned workflows

**Owners:** Homepage, Docs, Identity, Platform; shared gateway only composes
routes and authenticated navigation.

- [ ] Inspect sign-in, homepage, docs index, account, public app spaces, and
  private admin routes; record one concise audit per surface.
- [ ] Keep the existing Vapor visual character; establish reusable page/layout,
  type, spacing, status, table, form, and empty/error-state primitives in each
  service that owns them.
- [ ] Connect homepage, sign-in, account, app workspaces, and admin with
  explicit visible bridges and accurate signed-in/permission state.
- [ ] Restructure admin navigation by canonical task: approval inbox, deploy
  lifecycle, SSH access, host operations, recovery, and history. Do not merge
  service-owned records into a copied Platform database.
- [ ] Compact Approvals and Deployments while showing repository/revision,
  target, validity/order, owner, stage, retryability, and last result at the
  point of decision.
- [ ] Fix docs link/index freshness and ensure `/docs/` uses readable shared
  styling with accurate route and generated-content status.
- [ ] Review login, expired session, no permission, unavailable service, empty
  queue, stale revision, failed dispatch, and narrow viewport states.

## Phase 5: De-slop Vapor desktop surfaces

**Owner:** Vapor Client / Vapor-Studio / Vapor-GUI / Vapor-Installer.

- [ ] Audit Studio, Installer, account/sign-in, source chooser, content browser,
  operation center, and error/confirmation dialogs with the same direction.
- [ ] Consolidate repeated Egui spacing, type, surface, status, and action
  styles into the actual shared component/theme layer; remove decorative or
  repetitive copy, not useful controls.
- [ ] Align navigation, empty/error states, progress, and operation results
  with the web workflow names and real state transitions.
- [ ] Keep UI work inside its owning submodules and inspect their staged and
  unstaged changes before each edit.

## Phase 6: Local inventory cleanup and release checkpoint

- [ ] Search user home, AppData, Steam common, GitHub checkouts, and Vapor
  source/cache roots for known current and legacy Vapor locations.
- [ ] Compare manifests, Git ownership, exact file inventories, hashes, and
  generated-file status before deletion.
- [ ] Delete only confirmed generated/obsolete Vapor-owned copies; preserve
  uncommitted source, unsaved authored content, shared caches in use, game
  files, and user data unless full-nuke is explicitly invoked.
- [ ] Build Vapor for Linux and Windows; validate Installer, Developer tool
  readiness, update rollback, normal uninstall, and full-nuke fixture paths.
- [ ] Build and verify Packagepack release targets and Steam staging. Owner
  tests install/launch in Steam on Linux and Windows before public release.
- [ ] Review whole diff adversarially, commit by owning repository, push, and
  deploy only through the now-correct deployment workflow.

## Checkpoint notes

- Current Platform worktree is heavily dirty on branch
  `codex/server-network-observability-20261109`; do not reset it.
- Primary Client checkout is heavily dirty on
  `codex/standalone-runtime-targets`; nested submodules are dirty and partially
  staged. Keep each repo's changes separate.
- Loo Cast checkout on `codex/steam-depot-products` has an untracked
  `steam-mount/` and modified first-party manifests/README. This is the live
  Steam source candidate; preserve its authored changes while replacing the
  obsolete app-root contract.
- The Windows Packagepack release built successfully with the target-scoped
  static CRT setting. Import inspection found only standard Windows system
  components, so its depot payload needs no `libunwind.dll` sidecar. The Linux
  cross-build was attempted through the existing Zig 0.16 wrapper and stopped
  at `wayland-sys`: the Windows host has no `pkg-config` or Linux development
  sysroot. Keep Linux release verification open; solve it with a managed,
  reproducible Linux target environment or the native Linux builder, rather
  than weakening the Loo Cast Linux desktop feature set to make this host pass.
- The separate replacement/test VPS is reachable over its explicitly
  authorized SSH key and is a clean Ubuntu 26.04.1 x86_64 host. It currently
  has 1 vCPU, 1.8 GiB RAM, 55 GiB free disk, no swap, and no Rust or native
  development tools. It is a useful real setup/recovery test target, but a
  full release build there needs a low-memory build plan before provisioning.
- Steam app root currently contains multi-gigabyte generated `output`, content,
  tools and toolchains. No migration, delete, install, Steam staging, upload,
  or deployment was performed during inventory.
