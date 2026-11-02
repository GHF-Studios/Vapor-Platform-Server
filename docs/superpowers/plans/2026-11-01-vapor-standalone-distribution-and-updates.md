# Vapor Standalone Distribution and Updates Implementation Plan

> **Status: Superseded on 2026-11-02.** The owner-approved standalone product-boundary design changes the Steam app-root and Steam Workshop ownership model, and the target snapshot/native-builder plan now owns standalone install, website release, and self-update sequencing. Keep this file as historical context; do not execute it in parallel. Use [`2026-11-02-vapor-target-build-snapshots-and-builders.md`](2026-11-02-vapor-target-build-snapshots-and-builders.md) and the approved standalone product-boundary design instead.

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Publish the Vapor installer and client from the Vapor VPS and let installed copies update from Vapor's own public release feed without Steam, Workshop, Loo-Cast, or private alpha access.

**Architecture:** Treat Vapor application releases as a separate immutable release domain from Mad Max mods and content compositions. The VPS serves public version metadata and signed artifacts; a small trusted bootstrap verifies, stages, applies, and recovers client updates in Vapor's own app root. Existing Steam distribution remains a separate optional path for products that still need it.

**Tech Stack:** Existing Rust Vapor Client/Installer/GUI workspaces, the one-process Axum/Tokio server deployment, immutable release artifacts, and a cross-platform signature verification library selected after dependency review.

**Spec:** `docs/superpowers/specs/2026-11-01-vapor-independent-madmax-content-lifecycle-design.md`.

## Global Constraints

- The public Vapor installer, update metadata, and client releases come from Vapor's own VPS-controlled distribution path.
- Public download and update checks require no Steam or Loo-Cast login, ownership proof, alpha access, or Workshop subscription.
- Vapor app releases are distinct from Mad Max mod releases and from locally cached game data.
- Do not alter Loo-Cast's Steam App ID `2122620`, app root, or existing Steam publication flow.
- Release artifacts are immutable; corrections publish a new version.
- Verify authenticity and digest before staging; do not run an update artifact as an unverified installer.
- Never include Mad Max executable, archives, or extracted game assets in Vapor app releases.
- Keep the one-process, one-VPS deployment and explicit internal domain/state boundaries.
- Keep signing private keys off the server repository and outside ordinary VPS configuration.

## Review Focus

- **Tampered release metadata or bytes:** reject before staging and keep the installed Vapor version untouched.
- **Interrupted update:** recover to the prior working version or resume a verified staged update; never mark a partial update successful.
- **Downgrade or rollback attack:** reject an older version unless the user explicitly chooses a supported recovery action.
- **VPS or network unavailable:** installed Vapor remains usable; report update status without blocking local content management.
- **No Steam/Loo-Cast present:** first-run bootstrap and subsequent updates still succeed from a clean machine.

---

## Repository and File Map

### Vapor Client workspace

- `Vapor/crates/vapor_core/src/` — app-root/update transaction primitives if a stable reusable location exists; add a focused module rather than expanding unrelated app-root code.
- `Vapor-Installer/crates/vapor_installer/src/` — initial download/install bootstrap and recovery entrypoint.
- `Vapor-GUI/crates/vapor_gui/src/launcher.rs` and `crates/vapor_launcher_ui/src/lib.rs` — update availability, consent, progress, restart, and offline status in the standalone launcher.
- A new non-Steam standalone app-release manifest/packaging entrypoint belongs beside the existing root app manifests; never repurpose `App-Source.vapor.toml` or `App.vapor.toml` for Loo-Cast.

### Vapor Platform Server

- Consume the one-process host and immutable artifact boundaries from `docs/superpowers/plans/2026-11-01-vapor-catalog-server-execution.md`.
- Add an app-release route/module to the existing provider-neutral release/catalog composition or its separately owned module after the host mapping task identifies the exact source crate. Do not put release business logic in this orchestration root.
- Keep public artifact bytes in the server's immutable artifact store; metadata identifies the correct platform artifact and signature.

## Task 1: Define app release metadata and signing trust

**Files:**
- Read: the approved standalone ecosystem spec, the client/core release descriptor, the catalog-server plan, existing app-root manifests, and current signing/dependency policy.
- Create/update: a focused release contract under the owning Vapor/client and server documentation once their canonical path is confirmed.

**Interfaces:**
- Consumes: provider-neutral content-release concepts, but does not reuse a Mad Max mod `VaporId` as the identity of the Vapor application.
- Produces: `VaporAppRelease { version, channel, platform, minimum_updater_version, artifact_size, artifact_digest, signature, published_at }` and signed canonical bytes; app release identity is separate from mod content identity.

- [ ] Define canonical serialization and signature verification input; signature covers version, channel, platform, minimum updater version, size, digest, and immutable artifact location.
- [ ] Select a maintained cross-platform signing library after checking the existing workspace's cryptographic dependencies and licenses.
- [ ] Define public-key rotation and recovery: client trusts an embedded public root; private signing authority stays offline and outside source control/VPS deployment secrets.
- [ ] Define channel and downgrade behavior; first public release uses a stable channel and refuses silent rollback.
- [ ] Add contract cases for unknown channel/platform, malformed signature, version rollback, and artifact digest mismatch.

**Exit condition:** client and server have one versioned app-release contract that cannot be confused with Mad Max mod packages.

## Task 2: Serve public Vapor app releases from the one-process VPS

**Files:**
- Modify: the server-domain module and host route identified by the approved catalog-server plan's source mapping task.
- Modify: release artifact storage/metadata configuration without moving service business logic into the root orchestrator.
- Update: server route contract documentation and local integration fixtures.

**Interfaces:**
- Consumes: Task 1's signed `VaporAppRelease` descriptor and the immutable artifact store from the catalog-server plan.
- Produces: public latest-release lookup by channel/platform, exact immutable release lookup, and artifact download routes. No login is required to query or download public Vapor releases.

- [ ] Add route cases for available release, unsupported platform, no release, revoked release, and exact immutable artifact retrieval.
- [ ] Publish metadata only after the artifact is uploaded and its byte size/digest and signature verify.
- [ ] Keep app update routes distinct from mod catalog routes and Mad Max local-game endpoints.
- [ ] Verify unauthenticated download and authenticated/maintainer-only publication policy within the local one-process host.
- [ ] Verify route behavior using the server plan's local API test workflow; do not deploy to the VPS as part of this plan.

**Exit condition:** the public VPS release feed serves a signed Vapor client artifact and metadata without Steam or Loo-Cast credentials.

## Task 3: Add transactional self-update to the standalone Vapor app

**Files:**
- Create: focused update domain/provider modules in the Vapor Client workspace after checking its established module boundaries.
- Modify: standalone bootstrap and launcher update UI paths identified in Task 1.
- Do not modify the existing Loo-Cast Steam entrypoint or Steam app manifest.

**Interfaces:**
- Consumes: Task 1's verifier and Task 2's public release routes.
- Produces: update check, authenticated artifact download, staged update, apply/restart, rollback, and status operations rooted only in the Vapor app installation.

- [ ] Add tests for signature/digest rejection, unsupported OS/architecture, insufficient disk space, and server offline.
- [ ] Download to a temporary staging root with bounded size, verify before extraction, and reject path traversal or links escaping that root.
- [ ] Preserve the prior working release until the new release passes file verification and a successful startup health check.
- [ ] Recover an interrupted swap at next launch without touching the Mad Max directory or cache.
- [ ] Show update availability, release version, progress, restart choice, and failure/recovery state in the GUI; do not block offline gameplay/mod management on a failed update check.
- [ ] Run Vapor-managed focused checks and a disposable app-root update/rollback scenario.

**Exit condition:** a standalone Vapor installation updates itself from the VPS and can recover without Steam, Loo-Cast, or changes to the game files.

## Task 4: Publish the first standalone installer and release

**Files:**
- Create: public Vapor download/publishing workflow owned by the client release pipeline and server release contract.
- Update: Vapor Homepage download links and user-facing install/update documentation in their owning repositories.
- Update: the Mad Max local data workbench plan as a consumer of this standalone distribution.

**Interfaces:**
- Consumes: Tasks 1–3.
- Produces: public bootstrap installer, signed stable release metadata, platform artifact(s), and a documented no-Steam first-run path.

- [ ] Produce the bootstrap installer and stable app release with reproducible package contents and verifiable hashes/signatures.
- [ ] Confirm the artifact set contains no Loo-Cast-only entitlement check and no Mad Max game-derived files.
- [ ] Verify a fresh user can install Vapor, update it from the VPS, and use the launcher without Steam or Loo-Cast installed.
- [ ] Document uninstall and data-preservation behavior separately from Mad Max game/mod uninstall.

**Exit condition:** users can publicly download and self-update Vapor independently of Steam and Loo-Cast alpha access.

## Ordering

1. Task 1 defines the release/signature contract.
2. Task 2 is implemented inside the approved one-process server plan after its route/module mapping step.
3. Task 3 consumes the contract and public server routes.
4. Task 4 publishes only after signing, staging, and recovery behavior is validated.
5. The Mad Max local data workbench may be developed in parallel, but it is not considered distributable to players until this standalone bootstrap/update path is available.

## Self-Review

- Spec coverage: standalone public download and independence from Steam/Loo-Cast are Tasks 2–4; client self-update, signatures, and recovery are Tasks 1 and 3; game-file separation is a global constraint and tested in Tasks 3–4.
- Step scan: release contract precedes server and client implementations; server storage and client updater consume the same immutable metadata.
- Type consistency: `VaporAppRelease` is not a mod `ContentReference`; artifact signatures/digests are checked by the client before staging.
- Review focus: tampering, interruption, downgrade, offline operation, and no-Steam setup each have an owning task.
- Proportion: no publishing dashboard, multi-channel preview release, or VPS deployment migration is included.
