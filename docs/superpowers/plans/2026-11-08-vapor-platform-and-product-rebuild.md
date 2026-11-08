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
- Test archive verification and recovery-kit preflight on the USB. Record that the current task run verified the latest and one rotating archive; do not call all history verified.
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
- Steam Loo Cast content migration: inspect exact current `App.vapor.toml`, depot layout, Steam launch options, and the three source artifacts. Build a depot staging artifact containing only the three Loo Cast first-party products plus necessary Vapor manifest and optional locator. No Vapor app runtime, compiler toolchain, source checkout, cache, or user state in the Steam app root.
- Gate first-party product publication server-side. Validate that a user-edited manifest cannot alter product source, license, or publishability.
- Make standalone install work without Steam. Offer Loo Cast Steam launch as an optional locator into the installed Vapor selector; if Vapor is absent, show clear install guidance and stop. The Steam app root stays limited to `App.vapor.toml` and the final Packagepack artifact; do not add a loose locator binary without a new decision.
- **Exit:** clean Windows/Linux install + update + uninstall tests, Steam depot install/launch tests for developer and normal-player flows, and a dry-run/restoreable migration of the current power-user install.

### 4. Clean-room local migration and purge (Client)

- Inventory Steam app root, Steam workshop cache/manifest, Steam userdata/shortcuts/registry, `C:\Users\David Jackson\Vapor`, Local/Roaming AppData, PATH, tasks, and historical installer locations.
- Classify files as Steam-owned depot payload, Vapor-managed, user-authored checkout, game-owned, generated/cache, or unknown. Unknown paths default to preserve.
- Provide preview, export, and explicit clean-room deletion modes. Keep normal uninstall conservative. Full purge may delete Vapor-owned unsaved data and user-authored checkouts only after they are separately backed up and individually enumerated in the exact preview; unknown paths remain preserved.
- Re-home Vapor-managed files to documented user/system Vapor directories, then validate old paths are empty except the approved Steam manifest, content artifact, and optional locator.
- **Exit:** recorded before/after inventory with no licensed game files removed and no unreviewed data loss.

### 5. Optional worker and build cache (Platform + Build Cache)

- Measure the proposed second node and decide whether builds, replication, or both belong there. Do not require it for deploy.
- Define one locked toolchain and a separate signed cache snapshot per supported target. Local build fallback must follow the same locked manifest and signing/promotion contract.
- Test interrupted uploads, stale snapshots, node loss, and local fallback.
- **Exit:** worker failure drill proves local build and deploy still work; cache is reproducible and target-specific.

## Required gates before production reset or migration

1. Exact current backup on USB; fresh-host restore drill passed; backup provenance and plaintext-secret handling acknowledged.
2. Exact Steam and local path deletion/move manifest reviewed by owner; clean-room run has dry-run output and rollback export.
3. Public and admin user journeys tested in both Windows and Linux target flows where applicable.
4. Service-level ownership, audit, authorization, and independent deploy tests pass.
5. Adversarial review accepts the changes; no private keys/secrets in repos or logs.
6. Deployment itself is a visible, individually authorized action after source and rollback target are ready.

## Current verified checkpoint (2026-11-08)

- Main VPS exports remain available and an owner USB is mounted at `E:` (`BACKUPS`), with recovery files under `E:\VaporRecovery`.
- The hourly current-user task `Vapor USB Recovery Sync` is registered, uses the inbox Windows PowerShell executable, last returned success (`0`), and is scheduled for catch-up while the owner is signed in.
- Platform commit `31f192a4f0c6bb54e6c581e8b2d6471c7c951ab7` is live after its Linux and Windows CI passed. Its full deployment request `1ead570a-756e-4eef-aa1e-fc25d3239f01` completed, and all eight service health endpoints plus the public homepage, docs, downloads, and health routes returned success. The owner USB now records 44 archives and 211,370,133 archive-payload bytes; the 2026-11-08 11:09 UTC sync re-hashed every indexed archive, found zero unverified archives, and validated the recovery kit for `31f192a4f0c6bb54e6c581e8b2d6471c7c951ab7`.
- A one-time checksum sweep found no checksum failures; an older archive from the pre-root-ops-unit generation does not satisfy the current structural validator, so its historical kit is retained but is not a current-schema recovery kit.
- This is not a clean-host restore drill. No production wipe, Steam migration, clean-room purge, or VPS reset has been performed.
- The Windows full server test recipe passed on this checkout: Atlas validation/build, Python suite (112 passed, 10 skipped), Platform and submodule Rust tests. The matching GitHub Actions run passed both Linux and Windows jobs. Existing dead-code warnings remain in Diagnostics/Docs/Identity; no failures.
