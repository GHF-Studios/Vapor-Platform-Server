# Mad Max Local Data Workbench Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let Vapor Studio discover a player-owned Mad Max installation, browse its local asset inventory, and extract selected supported assets into a private rebuildable cache without uploading or modifying game files.

**Architecture:** Vapor owns generic installation state, local cache lifecycle, package/content references, and the GUI. A Vapor-managed Mad Max adapter owns game-specific archive/index knowledge, reusing the existing verified local asset workbench tools behind a versioned machine interface; proven hot paths can be ported to Rust later. The selected complete composition and one-live-install transaction remain governed by the existing standalone ecosystem plan.

**Tech Stack:** Existing Rust Vapor client/domain/provider/Studio crates, the Mad Max Rust project and its existing local asset-workbench tools, JSON metadata, and the existing Vapor-managed build/test workflow.

**Spec:** `docs/superpowers/specs/2026-11-01-vapor-independent-madmax-content-lifecycle-design.md`; also consume the approved umbrella spec and client/core plan under `MM-Building-System/docs/superpowers/`.

## Global Constraints

- Mad Max executable, archives, and extracted game assets remain local and are never uploaded to Vapor.
- Do not change the live game directory or game archives in this plan; all extraction is read-only from the game and writes only to Vapor's cache.
- Steam may help locate an installation but must not be required for installation discovery, catalog access, or launch.
- Do not depend on Loo-Cast, Steam App ID `2122620`, its app root, depots, or content.
- Vapor itself is publicly downloadable from Vapor's VPS and updates through Vapor's own release feed; Steam/Loo-Cast alpha access is never an entitlement gate.
- Use the stable Vapor identity and prerequisite contracts from `Vapor#2` and `Vapor#4`; do not create temporary IDs or duplicate their migrations.
- Preserve a single-process, one-VPS Vapor server deployment with explicit internal domain boundaries as selected by the existing standalone ecosystem workstream.
- Ordinary user actions remain in the Vapor GUI; any helper process is a managed internal adapter, not a required user-facing CLI.
- Build and validate through Vapor-managed commands from the owning workspace; do not invoke raw Cargo.

## Review Focus

- **No local game selected:** the workbench explains that no source is configured and remains usable without Steam.
- **Game update or changed archives:** stale local indexes/cache entries are marked invalid and are never silently treated as current.
- **Untrusted manifest paths:** absolute paths, traversal, symlinks escaping the cache, and malformed archive-member references are rejected before extraction.
- **Private local path leakage:** exported/index metadata and diagnostics omit absolute game/cache paths and extracted payload bytes.
- **Unsupported format or decoder:** the UI reports “indexed but not previewable/extractable” rather than implying that the asset is corrupt or supported.

---

## Repository and File Map

### Vapor Client workspace

- `Vapor/crates/vapor_client_domain/src/` — typed local installation/cache records, separate from published content identity.
- `Vapor/crates/vapor_client_provider/src/` — local filesystem and Mad Max adapter boundary; no server-side game-file processing.
- `Vapor-Studio/crates/vapor_studio_ui/src/pages/` — local asset workbench page and navigation integration.
- `Vapor-Studio/crates/vapor_studio_ui/src/components/` — reusable inventory/tree/detail widgets where existing patterns fit.

### Mad Max project

- `tools/asset_workbench/README.md` — current local tool contract and verified support limits.
- `tools/asset_workbench/build_manifest.py` — current metadata-only inventory source.
- `tools/asset_workbench/extract_resource.py` and `extract_bundle_member.py` — current bounded local extraction implementations.
- `tools/asset_workbench/prepare_preview.py` and format inspectors — current read-only preview/decode support.
- `target/content-index/` — ignored generated output only; never a committed or distributable cache location.

Existing source inspection found the current asset workbench is local, extracts selected resources with size limits and provenance, and writes ignored outputs under `target/content-index/`. Its format coverage is explicitly incomplete; the Vapor workbench must preserve those capability labels and must not imply universal decoding.

## Task 1: Lock the local installation and cache contracts

**Files:**
- Read: `Vapor/crates/vapor_content/src/identity.rs`, `Vapor/crates/vapor_content/src/catalog.rs`, and `Vapor/crates/vapor_client_domain/src/composition.rs`.
- Create: `Vapor/crates/vapor_client_domain/src/game_installation.rs`.
- Modify: `Vapor/crates/vapor_client_domain/src/lib.rs` and the owning crate's test modules.

**Interfaces:**
- Consumes: stable Vapor content identity/version contract from `Vapor#2` and supported-target evidence model from `Vapor#9`.
- Produces: serializable `GameInstallation { id, game: ContentReference, root: PathBuf, executable: PathBuf, fingerprint, validation }`, `GameBuildFingerprint`, `LocalAssetCacheRecord`, and `LocalAssetReference { game, build, logical_path, source_archive, archive_member }` types. The installation path is local state and is never part of a public content ID. Local asset references identify a game build plus normalized logical asset/archive-member identity, not an absolute path.

- [ ] Confirm the active identity and target-support issues expose stable contracts; if not, leave implementation blocked and update only the plan interfaces.
- [ ] Add round-trip tests for a manually selected installation and for cache metadata keyed to a supported game build and adapter version.
- [ ] Add tests for missing executable, changed build fingerprint, stale cache version, and path changes.
- [ ] Implement the typed local state without overloading package `InstallationState` or adding game bytes to content identity.
- [ ] From the Vapor Shell with the Vapor workspace selected, run `test --project all` and confirm the new domain tests pass.

**Exit condition:** Vapor can persist a local game target and identify whether its local asset cache is valid without representing the game as a downloadable package.

## Task 2: Export a stable local asset inventory from the Mad Max adapter

**Files:**
- Create: `tools/asset_workbench/export_vapor_index.py`.
- Create: `tools/asset_workbench/vapor_adapter.py`.
- Modify: `tools/asset_workbench/README.md`.
- Read: `tools/asset_workbench/build_manifest.py`, `extract_resource.py`, `extract_bundle_member.py`, and generated manifest records under ignored `target/content-index/`.

**Interfaces:**
- Consumes: current local `master.dirlist`-derived census, installed archive tables, parsed bundle/member metadata, and viewer capability metadata.
- Produces: a versioned machine-readable index with normalized logical references, source archive/member identity, game-build fingerprint, adapter/index version, asset family, and per-operation capability flags. It contains no payload bytes and no absolute local paths. `vapor_adapter.py` accepts a versioned JSON request on stdin and emits one JSON response on stdout for `index` and exact-resource `extract` operations; diagnostics go to stderr.

- [ ] Add exporter cases for a loose logical path, an archive resource, a bundled `.ee` member, duplicate logical paths with distinct source archives, and a resource with no supported viewer.
- [ ] Reject paths that escape the logical game namespace and omit cache-local output paths from the exported records.
- [ ] Generate the index from existing manifest inputs without changing game archives or extracting every payload.
- [ ] Add the JSON-lines adapter contract for listing and extracting one exact `LocalAssetReference`; extraction responses include bounded output size and provenance. The adapter rejects unknown protocol versions and unknown operation names.
- [ ] Verify exporter output is deterministic for identical inputs and contains metadata only.

**Exit condition:** the Mad Max adapter can provide an exact, bounded, local-only asset inventory and on-demand extraction contract that Vapor can consume.

## Task 3: Add the local game-data provider and cache manager

**Files:**
- Create: `Vapor/crates/vapor_client_provider/src/game_data/mod.rs`.
- Create: `Vapor/crates/vapor_client_provider/src/game_data/madmax.rs`.
- Modify: `Vapor/crates/vapor_client_provider/src/lib.rs` and the client-domain/provider test modules.

**Interfaces:**
- Consumes: Task 1's `GameInstallation` and `LocalAssetReference`; Task 2's versioned Mad Max adapter contract.
- Produces: local operations to validate a selected game path, refresh its metadata index, resolve a local asset reference, extract one supported asset into a Vapor-owned cache, and report cache provenance/status. No operation mutates the source installation.

- [ ] Add provider tests for no installation, a valid install, a game build change, absent source asset, stale adapter version, extraction-size limit, and cache hit.
- [ ] Implement path validation and source fingerprinting without requiring Steam discovery.
- [ ] Implement atomic index/cache updates: a failed scan or extraction leaves the last verified cache state intact.
- [ ] Ensure cache paths are canonicalized under the Vapor cache root and adapter outputs cannot escape it.
- [ ] Keep any process invocation hidden behind the provider API; the UI must not construct shell commands or ask the player to run CLI steps.
- [ ] From the Vapor Shell with the Vapor workspace selected, run `test --project all` and confirm the provider/domain checks pass.

**Exit condition:** the client can refresh a local inventory and extract a selected supported asset into a rebuildable cache, with no game-directory writes and no network upload.

## Task 4: Add a focused Local Assets workbench to Vapor Studio

**Files:**
- Create: `Vapor-Studio/crates/vapor_studio_ui/src/pages/game_assets.rs`.
- Modify: `Vapor-Studio/crates/vapor_studio_ui/src/pages/mod.rs`, navigation, and the Studio application module that owns page dispatch.
- Reuse: existing library/catalog list and content-detail components where their semantics fit.

**Interfaces:**
- Consumes: the local installation and game-data provider operations from Tasks 1 and 3.
- Produces: a GUI page to select/validate the game directory, refresh the local index, search/filter assets, inspect provenance/capabilities, and request a preview/extraction for supported resources.

- [ ] Add UI-state tests for no target, scan in progress, valid/invalid target, empty inventory, stale index, unsupported preview, and extraction failure.
- [ ] Implement an efficient virtualized or paged inventory view; do not load asset payloads while scrolling/searching.
- [ ] Show clear local-only/read-only provenance and separate “indexed”, “cached”, “previewable”, and “editable” states.
- [ ] Open only supported cached previews; never report an unsupported format as a successful preview.
- [ ] Verify the page remains usable with the catalog offline and without a Steam installation.

**Exit condition:** a player can use the GUI to browse local game data and preview at least the currently verified text/image/mesh cases without using a separate CLI workflow.

## Task 5: Wire local assets into compositions without distributing them

**Files:**
- Modify only the client composition/package resolver paths identified by the existing client-core plan; do not change the server orchestration root.
- Update the existing provider-neutral catalog contract only if a local-only reference cannot be represented without placing a machine path in public metadata.

**Interfaces:**
- Consumes: Tasks 1–4 and the existing immutable release/composition contract.
- Produces: a resolver distinction between downloadable mod-authored package files and local-only game asset references. Local references are checked against the selected game's build and index; they never become remote package blobs.

- [ ] Add resolver tests showing a complete composition may reference a locally available asset while the catalog release contains no base-game bytes.
- [ ] Add failure cases for missing local asset, wrong game build, stale cache, and a malicious package attempting to include an absolute game path.
- [ ] Resolve local references before creating an install preview; report missing assets without writing to the live game directory.
- [ ] Reuse the already approved receipt-backed install plan for any future live-directory operation; this task only resolves and previews.
- [ ] Confirm public package output contains mod-authored distributable files and no local cache paths or extracted game payloads.

**Exit condition:** a published composition can declare local Mad Max asset requirements while all proprietary bytes stay on the player's device.

## Execution Order and Gates

1. Task 1 waits on the canonical identity/target contracts already owned by the open Vapor issues; do not create parallel identifiers.
2. Task 2 can be developed against current local tool fixtures while Task 1 is gated.
3. Tasks 3 and 4 follow Task 1 and Task 2. They should be implemented in the Vapor client feature workspace, not in the server orchestration repository.
4. Task 5 follows the provider-neutral release contract and remains a dry-run/resolution feature; actual composition installation is covered by the existing receipt-backed client/package plans.
5. Server catalog/API and one-process deployment remain covered by `2026-11-01-vapor-catalog-server-execution.md`; no game file extraction or cache operation belongs on the VPS.
6. Standalone Vapor bootstrap and self-update are a distinct app-release lifecycle; they must be available before this workbench is distributed and are not Mad Max mod packages.

## Self-Review

- Spec coverage: local installation, local extraction/cache, read-only game source, composition references, and GUI asset browsing are Tasks 1–5; server/catalog boundaries remain in the approved server plan.
- Step scan: each task establishes an interface for the next and has bounded failure cases and an exit condition.
- Type consistency: `GameInstallation` stores local target state; `LocalAssetReference` is not a public `VaporId`; downloaded release artifacts remain separate from game-derived cache entries.
- Review focus: missing target, changed game build, unsafe paths, local path leakage, and unsupported decoders each have a task-owned check.
- Proportion: this plan does not include archive repacking, live game mutation, generalized asset editing, or server deployment; those require later format-specific plans.

## Execution Handoff

This is a supplementary local-content plan; the already approved umbrella,
client/core, catalog-server, and first-package plans remain authoritative for
standalone distribution, the one-process server, and reversible game-folder
installation. Implementation method is inline/native as requested; do not use
subagents. Source changes belong in the Vapor Client and Mad Max repositories,
not this server checkout. Before execution, the implementation workspace must
make those repositories writable and the active `Vapor#2` / `#4` contracts must
be rechecked.
