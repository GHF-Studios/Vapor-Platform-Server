# Vapor Platform and Product Rebuild Plan

**Status:** Executable staged plan for the owner-approved rebuild charter dated 2026-11-08.
**Rule:** Work in owner repositories; preserve dirty checkouts. No deployment or destructive purge is implied by this plan.

## Work lanes and order

### 0. Freeze the evidence and source ownership

- Capture current Platform/service SHAs, live host topology, Steam app/depot state, and exact local install path inventory.
- Inventory dirty worktrees and nested submodules without resetting, stashing, moving, or deleting them. Assign each proposed change to its owner repo.
- Mark older conflicting architecture documents superseded; do not silently rewrite historical evidence.
- **Exit:** reproducible inventory and no ambiguous edit ownership for the first implementation tranche.

### 1. Finish recovery baseline (Platform)

- Keep hourly state export and the removable-drive sync operational; make the backup status easy to read and alert on stale/failed copies.
- Re-hash every indexed USB archive on each sync, repair damaged indexed copies from the VPS, and preserve/report unindexed files. Record counts and recovery-kit preflight results; never describe an archive as verified without hashing its bytes against the current VPS index.
- Produce a redacted rebuild checklist from a genuinely fresh Ubuntu host: OS assumptions, source pin, service setup, secrets/config import, external DNS/provider steps, health criteria, rollback boundaries.
- Run a restore drill on a disposable VPS/host before any production wipe. Verify every owner service's state, Caddy routes, login/auth flows, exact source/submodule pins, then create and verify a repeat export.
- Any `platform state reset` preview/execute workflow must name its scope and must not imply a host reinstall. Full-host compromise recovery is a separate runbook: Strato OS reinstall (manually through Strato if no safe API exists), owner SSH key reauthorization, then pinned recovery-kit import. Both require a fresh validated export and exact confirmation.
- **Exit:** successful clean-host drill and owner-reviewed data-retention report.

### 2. Rebuild the Platform admin experience (Platform + owner services)

- Map each top-level operator task to one canonical service and API: approvals, deployments, temporary SSH, Identity access, service health, backups/recovery, and history.
- Separate current actionable queues from execution records and historical audit. A saved approval, queued operation, running operation, and terminal result must remain distinct with one visible correlated operation ID.
- Keep the shared gateway shell/navigation; use service-owned fragments/APIs and owner-owned audits. Define a stable shared component contract only after the current fragment contract is documented and tested.
- Replace giant catch-all pages with task workbenches: `Approvals`, `Deployments`, `Temporary Access`, `Identity`, `Services`, `Recovery`, and `History`. Keep only actionable summary cards on the landing page. Do not duplicate one request in several queues without clear links/state.
- SSH UI requests public-key authorization from requester-side generated keys; show fingerprint, requested scope/duration, granted scope/duration, account, expiry, and audit. No private-key display. The one-host root emergency browser console is a separate high-security workbench with its own short-session and origin/step-up gates; it is not part of ordinary SSH requests and must stay disabled until those gates are satisfied.
- Audit every button against API semantics, authorization, retries/idempotency, and visible result. Add keyboard, focus, narrow viewport, loading, empty, partial, forbidden, and service-down states.
- **Exit:** end-to-end browser tests for each workflow and an adversarial security review of privilege boundaries.

### 3. Public Vapor and product workbenches (Client + product repos + services)

- Standalone Vapor website download, signed updater, install/uninstall, getting-started and working docs. Steam is not required to download/install Vapor.
- Product adapters/workbenches: Mad Max (local game detection and mod receipts), native Loo Cast (depot content), and isolated Sandbox. Keep their legal/source semantics distinct while reusing widgets.
- Steam Loo Cast content migration: inspect exact current `App.vapor.toml`, depot layout, Steam launch options, and the three source products. Build one self-contained executable depot artifact that embeds `App.vapor.toml` and all three first-party products. No separate product folders, loose locator, Vapor app runtime, compiler toolchain, source checkout, cache, or user state in the Steam app root.
- Gate first-party product publication server-side. Validate that a user-edited manifest cannot alter product source, license, or publishability.
- Make standalone install work without Steam. If the Steam artifact offers a Vapor tools launch action, implement it within the self-contained executable/embedded manifest contract; if Vapor is absent, show clear install guidance and stop. The Steam app root stays limited to the single self-contained executable artifact, which embeds the manifest and products.
- **Exit:** clean Windows/Linux install + update + uninstall tests, Steam depot install/launch tests for developer and normal-player flows, and a dry-run/restoreable migration of the current power-user install.

### 4. Clean-room local migration and purge (Client)

- Inventory Steam app root, Steam workshop cache/manifest, Steam userdata/shortcuts/registry, `C:\Users\David Jackson\Vapor`, Local/Roaming AppData, PATH, tasks, and historical installer locations.
- Classify files as Steam-owned depot payload, Vapor-managed, user-authored checkout, game-owned, generated/cache, or unknown. Unknown paths default to preserve.
- Provide preview, export, and explicit clean-room deletion modes. Keep normal uninstall conservative. Full purge may delete Vapor-owned unsaved data and user-authored checkouts only after they are separately backed up and individually enumerated in the exact preview; unknown paths remain preserved.
- Re-home Vapor-managed files to documented user/system Vapor directories, then validate the Steam app root contains only the single approved self-contained executable artifact (with its embedded manifest/products) and Steam-owned files.
- **Exit:** recorded before/after inventory with no licensed game files removed and no unreviewed data loss.

### 5. Optional worker and build cache (Platform + Build Cache)

- Measure the proposed second node and decide whether builds, replication, or both belong there. Do not require it for deploy.
- Keep the service concept precise: Build Cache stores signed Cargo target-directory snapshots, not `sccache` compiler entries and not build-job state. Platform owns durable build-operation records and executor lifecycle; Artifact owns signed application releases.
- Define one locked toolchain and a separate signed snapshot per supported target. Local build fallback must remain available and use the same canonical lock/recipe inputs; it does not implicitly publish a snapshot.
- Define the second node as an optional executor/cache accelerator only after measuring it. A missing worker must not block local source builds, service deploys, or host recovery. Keep any shared compiler-result cache separately keyed and optional.
- Test interrupted uploads, stale snapshots, node loss, and local fallback.
- Record build attempts, executor assignment, state transitions, cancellation, bounded logs, returned candidate identity, and cleanup under a durable Platform operation ID; Build Cache's current pointer is only the latest successful promotion, not job history.
- **Exit:** worker failure drill proves local build and deploy still work; cache is reproducible and target-specific.

## Required gates before production reset or migration

1. Exact current backup on USB; fresh-host restore drill passed; backup provenance and plaintext-secret handling acknowledged.
2. Exact Steam and local path deletion/move manifest reviewed by owner; clean-room run has dry-run output and rollback export.
3. Public and admin user journeys tested in both Windows and Linux target flows where applicable.
4. Service-level ownership, audit, authorization, and independent deploy tests pass.
5. Adversarial review accepts the changes; no private keys/secrets in repos or logs.
6. Deployment itself is a visible, individually authorized action after source and rollback target are ready.

## Current verified checkpoint (2026-11-09 01:07 UTC)

- Platform source `0b21c964df4f08a5ab5c99f893548ead199ef67d` is on `main`; the
  currently deployed composition remains
  `78722c2641b42bfdf493d45b6b92461dccd3e87f`, completed at
  `2026-11-09T00:47:09Z`. The deployed-source inventory pins Platform and all
  eight service targets to that composition. Platform, Homepage, Docs,
  Identity, Diagnostics, Registry, Artifact, Build Cache, and Caddy are active.
- The three old failed deployment systemd units were confirmed against their
  retained Platform operation records and cleared from systemd's failed-unit
  list. Their Platform history was not changed.
- The owner USB is the freshly formatted `E:` volume (`BACKUPS`). Its
  `VaporRecovery` root contains the state snapshots and recovery kit. The
  scheduled `Vapor USB Recovery Sync` completed at `2026-11-09T01:07:48Z` with
  result `0`; all eight indexed archives and every recovery-kit file hash were
  verified. Total snapshot payload is 38,389,881 bytes.
- The newest USB archive is
  `vapor-platform-state-20261109T010116Z-34f63ec46441.tar.gz` with SHA-256
  `edd21e88ec907b1108914f15561e0201629af14c4b865cd29da876c3bc9a53e9`. It
  records the deployed Platform source `78722c2641b42bfdf493d45b6b92461dccd3e87f`;
  the recovery-tool kit is independently pinned to
  `cbcf388536fdce8c18e852a8950fd6cabed5acee`.
- A clean-host restore is not proven. This Windows workstation has no Linux
  hypervisor/WSL environment available, and the disposable Ubuntu host is not
  ready. Do not infer a successful restore from the archive validation.
- Build Cache reports both supported target triples as unconfigured and without
  a current snapshot. No optional worker is configured. The available
  `Vapor-Client` checkout is still dirty and has no canonical ecosystem
  `Vapor.lock` or toolchain lock, so no official snapshot identity/recipe can
  safely be invented. Keep local source builds as the fallback and preserve
  the dirty checkout.

## Steam app-root inventory and payload decision (2026-11-09 11:58 UTC)

- Read-only inventory of the installed Loo Cast directory found 39,711,106,627
  bytes (39.71 GB decimal) across the Steam app root. The largest entries are
  `output` (28,781,730,980 bytes), `rustup-home` (3,772,556,115), `content`
  (4,287,400,175), `cargo-home` (915,911,545), and `tools` (1,109,175,632).
  Vapor build/cache/toolchain data therefore dominate the local footprint.
- Steam's `appmanifest_2122620.acf` lists only depots `2122621` and `2122623`,
  sized 75,298,219 and 243,241,642 bytes. Their 318,539,861-byte sum exactly
  matches Steam's `SizeOnDisk`; the remaining roughly 39.39 GB is outside
  Steam's recorded depot payload. This makes the current mixed-ownership app
  directory measurable rather than an assumption.
- The installed root still has a loose `App.vapor.toml`; it names three
  Workshop IDs for the Spacetime Engine, Loo Cast Game, and Loo Cast
  Packagepack. That manifest does not reflect the owner's stated removal of
  those Workshop items and must not be reused unchanged for the replacement.
- Confirmed target: one self-contained executable artifact in the Steam depot,
  embedding `App.vapor.toml` and the Engine, Game, and Packagepack payloads.
  Steam must not contain a standalone Vapor runtime, compiler/toolchains,
  build outputs, caches, checkouts, or user state. The executable may hand off
  to a separately installed Vapor tools selector; it must not bundle Vapor.
- This inventory did not move or delete anything. Migration remains a
  previewable, exportable Client-owned operation; the exact source files and
  package contents need classification before a clean-room purge.

## Historical checkpoint (2026-11-08)

- Main VPS exports remain available and an owner USB is mounted at `E:` (`BACKUPS`), with recovery files under `E:\VaporRecovery`.
- The hourly current-user task `Vapor USB Recovery Sync` is registered, uses the inbox Windows PowerShell executable, last returned success (`0`), and is scheduled for catch-up while the owner is signed in.
- Platform commit `31f192a4f0c6bb54e6c581e8b2d6471c7c951ab7` is live after its Linux and Windows CI passed. Its full deployment request `1ead570a-756e-4eef-aa1e-fc25d3239f01` completed, and all eight service health endpoints plus the public homepage, docs, downloads, and health routes returned success. The owner USB now records 44 archives and 211,370,133 archive-payload bytes; the 2026-11-08 11:09 UTC sync re-hashed every indexed archive, found zero unverified archives, and validated the recovery kit for `31f192a4f0c6bb54e6c581e8b2d6471c7c951ab7`.
- A one-time checksum sweep found no checksum failures; an older archive from the pre-root-ops-unit generation does not satisfy the current structural validator, so its historical kit is retained but is not a current-schema recovery kit.
- This is not a clean-host restore drill. No production wipe, Steam migration, clean-room purge, or VPS reset has been performed.
- The Windows full server test recipe passed on this checkout: Atlas validation/build, Python suite (112 passed, 10 skipped), Platform and submodule Rust tests. The matching GitHub Actions run passed both Linux and Windows jobs. Existing dead-code warnings remain in Diagnostics/Docs/Identity; no failures.
- Project-centered account workspaces were fast-forwarded to the service and Platform `main` branches at Identity `2b0db42`, Registry `ea6ae62`, Homepage `caf61e7`, and Platform `cd19862`; composed Platform CI passed on Linux and Windows. Full deployment request `eb913a8e-5b51-44e4-862c-4e8139b0271f` completed successfully. The live checkout is `cd19862`; all eight Vapor service units and Caddy are active. Smoke checks returned 200 for public homepage, `/dashboard`, `/login`, and Project workspace shell, and 401 for the unauthenticated Project API.
- After deployment, the VPS state export was run manually. The owner USB sync then copied and rehashed the new `vapor-platform-recovery-20261108T164015Z-cd19862c3b5c.tar.gz` archive; the local SHA-256 matches the VPS index. The 16:40:32 UTC sync reports 51 archives, 245,097,331 payload bytes, zero unverified archives, and a preflight-validated recovery kit sourced from `cd19862`. The previous no-drill limitation remains: this proves archive freshness and integrity, not a clean-host restore.
