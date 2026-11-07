# Vapor Independent Platform and Mad Max Local Content Lifecycle

Status: approved for implementation on 2026-11-01. Implementation must follow
the approved repository-specific plans and preserve their owner-controlled
  deployment and publication gates. Its one-process VPS assumption was
  superseded by the independent-service topology in
  [`2026-11-07-independent-service-deployments.md`](2026-11-07-independent-service-deployments.md).

## Intent

Vapor is an independently operated content platform, hosted on Vapor's VPS and
not dependent on Loo-Cast or Steam for distribution, accounts, catalog access,
or launching a game. The standalone Vapor installer and client are downloaded
from Vapor's own server and receive updates through Vapor's own release feed;
Steam and Loo-Cast alpha access grant no Vapor entitlement. Mad Max is one
ecosystem hosted by Vapor. Each player
acquires Mad Max through a legitimate local source; Vapor never distributes the
game or its proprietary data.

Vapor should let a player inspect and manage their local Mad Max installation,
build a local cache of extracted/indexed game data, select a complete content
composition, and launch the one live game installation with that composition
applied. Switching compositions must be reversible and recoverable.

## Goals

- Keep Vapor's public platform, catalog, account lifecycle, and distribution
  independent of Steam and Loo-Cast.
- Offer a public standalone Vapor download and self-update path hosted by the
  Vapor VPS, separate from game/mod package releases and requiring no Steam or
  Loo-Cast access.
- Keep Mad Max executable, archives, extracted assets, and other proprietary
  game data on the player's machine.
- Provide a local, rebuildable cache for extracted/indexed data and Mad Max
  tools such as archive inspection and asset viewers/editors.
- Allow the player to choose and install complete compositions as the current
  playable setup, with a reliable route back to vanilla.
- Make profile switching transactional: interruption or failure must be
  detectable and recoverable without guessing which files belong to which
  composition.
- Keep platform service responsibilities and state boundaries explicit across
  independently deployable services on the VPS; the Platform root remains
  responsible for deployment/orchestration rather than absorbing service
  business rules.

## Non-goals

- Hosting, mirroring, or redistributing Mad Max or extracted proprietary game
  assets.
- Designing custom-composition authoring/composition capabilities in this
  tranche. Players can install published complete compositions; richer
  composition tools can follow.
- Requiring a separate full Mad Max installation for every composition.
- Moving archive parsing, asset editing, or local filesystem operations onto
  the VPS.
- Rebuilding Vapor as a Loo-Cast feature or restoring Steam Workshop as its
  required content provider.
- Implementing a specific account login method in this content-lifecycle
  design.

## Current facts and mismatch

The current `Vapor-Platform-Server` documentation describes separate service
repositories for Homepage, Docs, Identity, Diagnostics, and Registry, with the
root repo owning routing, deployment, and composed backup/restore. The approved
Mad Max/Vapor workstream selects one server process on one VPS while retaining
those domain boundaries. The root `AGENTS.md` and older server docs still state
an independently deployed process topology; that documentation conflict must
be reconciled explicitly as part of the approved monolith migration, not
silently worked around.

The current identity model is still Steam-anchored, and prior Vapor Client
configuration binds a distribution path to a Steam application. Those are
implementation facts, not requirements for this design. A Steam login,
Steam-launched client, Workshop provider, or Steam ownership check must not be a
prerequisite for Vapor or the Mad Max ecosystem. If Steam identity remains
available, it is an optional linked provider only. The exact first-party Vapor
account/authentication migration is a separate decision.

## Approaches considered

1. **Rebuild the live game directory on every switch.** Restore baseline files,
   then copy the selected composition into the game directory. This is simple
   and storage-efficient, but a crash midway can leave a partially switched
   installation.
2. **Stage and transactionally reconcile one live game directory — selected.**
   Keep the game as locally acquired, maintain a verified local baseline/cache,
   prepare a complete desired file state away from the live directory, then
   apply the delta as a journaled transaction. This balances disk use with
   reliable rollback and composition switching.
3. **Keep a separate full game copy per composition.** This isolates each setup
   and makes switching straightforward, but duplicates a large installation
   and makes game updates and storage management cumbersome.

Approach 2 was selected because it preserves one live game installation while
making the installed composition explicit and recoverable.

### Vapor application distribution

1. **Steam/Workshop distribution.** Rejected because it makes Steam and the
   Loo-Cast alpha app root an access gate for Vapor.
2. **Vapor-hosted manual downloads only.** This removes Steam entitlement but
   leaves updates dependent on users finding and installing each new build.
3. **Public Vapor-hosted bootstrap plus signed self-update feed — selected.**
   Vapor publishes its own immutable app releases and signed update metadata on
   its VPS. The installer bootstraps the client; the client checks that feed,
   verifies authenticity and digest, stages an update, and can recover from an
   interrupted update. Public discovery and updates require no Steam or
   Loo-Cast login. This gives Vapor an independent distribution lifecycle while
   keeping the existing mod catalog a separate content domain.

## Ownership boundaries

### Vapor Platform Server

- Serves Vapor-owned account, catalog, release metadata, and distributable
  mod-authored packages through explicitly separated service-domain modules.
- Publishes Vapor desktop application release metadata and immutable client
  artifacts through a separate application-update contract; this is distinct
  from Mad Max mod releases.
- Makes public Vapor downloads and update metadata available without Steam,
  Workshop, or Loo-Cast entitlement.
- Does not receive or store Mad Max installations, extracted assets, player
  files, or local extraction caches.
- Does not perform game archive extraction, repacking, patching, or launch.
- Keeps the root repository responsible for deployment/orchestration and
  whole-system export/import composition, not domain-specific catalog or asset
  business logic. Services run independently behind the shared gateway and
  retain their own modules and state ownership.

The existing Registry is the likely home for provider-neutral ecosystem and
composition metadata, subject to its own contract and implementation review.
If binary distribution later needs a dedicated artifact service, that service
gets its own boundary rather than being folded into the orchestration root.

### Vapor Client / local tools

- Detects or lets the player select a legitimate local Mad Max installation.
- Validates the executable/build and captures a local baseline manifest before
  managing files.
- Runs Mad Max-specific archive, indexing, extraction, preview, and editing
  tools locally.
- Stores extracted/indexed outputs in a rebuildable local cache keyed by game
  build, source-file identity, and tool version.
- Downloads only Vapor-published metadata and legally distributable mod
  packages; resolves local-only game references from the user's cache.
- Prepares, applies, verifies, rolls back, uninstalls, and launches the selected
  composition against the one live game directory.
- Bootstraps and updates Vapor itself from the Vapor VPS after verifying the
  signed update metadata and release artifact; the updater must not use Steam
  depots or the Loo-Cast app root.

The implementation may be a local platform adapter or a separately packaged
Mad Max tool component. It must integrate with Vapor's lifecycle without
turning game-specific processing into server business logic.

### Mad Max ecosystem content

- A published complete composition declares its identity/version, supported
  game builds, mod package dependencies, install operations, and any required
  local game assets by stable local references (for example archive path plus
  entry name and expected hash where available).
- Mod-authored distributable files are separate from local game-derived files.
- A package must not silently embed or upload proprietary base-game files.
- Asset viewers/editors operate on local cached data and clearly distinguish
  read-only game sources from authored or generated output.

## Local file lifecycle

```text
Player-owned game installation
          |
          +--> validate build and record baseline manifest
          |
          +--> local extractor/indexer --> rebuildable local asset cache
          |
Vapor catalog --> composition manifest + mod-authored packages
          |
          v
Local resolver validates all package and local-asset references
          |
          v
Stage complete desired game-directory state outside the live directory
          |
          v
Journaled apply --> verify --> mark composition active --> launch local game
          |
          +--> rollback / restore baseline / switch to another composition
```

The baseline and composition manifest must distinguish at least:

- original files observed in the locally acquired game;
- files generated in the local cache;
- files supplied by Vapor packages;
- files replaced, added, or removed in the live directory;
- the active composition and transaction state.

Before the local transaction format is specified, the Mad Max adapter must
inventory actual archive and loose-file behavior. The design does not assume
that game archives can be safely overlaid at runtime. If a format requires
repacking or replacement, that is a local adapter operation with backup,
validation, and rollback semantics.

## Composition switching contract

1. Resolve and validate the selected complete composition and all dependencies
   before touching the live game directory.
2. Verify that referenced local game assets exist in the cache or can be
   extracted from the selected local installation.
3. Build a staged desired state and produce a human-readable change summary.
4. Record a durable transaction journal and preserve recoverable originals
   before applying mutations.
5. Apply the transaction; verify expected outputs; only then mark the
   composition active.
6. On interruption, detect the incomplete transaction at next launch and offer
   recovery or automatic rollback; never claim a composition is active until
   verification succeeds.
7. Switching to vanilla means restoring the recorded local baseline, not
   downloading a game copy from Vapor.
8. Uninstalling Vapor management removes only Vapor-owned files/state and
   restores managed game files to their captured baseline when possible.

Game updates invalidate or revalidate the baseline and cache based on observed
build/file changes. Vapor must not overwrite a changed game installation using
stale baseline data without detecting and reporting the conflict.

## Content and legal boundary

- Game files are acquired by the player from their own legitimate source.
- Proprietary files and extracted game assets stay local and are never
  published to Vapor's server or included in a distributable composition.
- Vapor may distribute manifests, metadata, tools, and mod-authored content
  whose author has distribution rights.
- The local client must reject or warn on package contents identified as
  copied game data where the package format can detect that reliably; this is
  defense-in-depth, not a claim that automated detection is complete.
- Logs, crash reports, and support bundles must not include game assets or
  extracted content by default.
- Vapor's own client binaries, installer, and update metadata are Vapor
  releases, not Mad Max content; the server may distribute them under their
  applicable licenses and provenance.

## Open design edges for implementation planning

- Provider-neutral Vapor identity/account migration, including whether Steam
  remains an optional link.
- Vapor app-release signing key custody, trust-root rotation, update channels,
  anti-downgrade policy, and recovery behavior for interrupted self-updates.
- Exact Registry schema/API for ecosystems, compositions, releases, and local
  asset references.
- Local package format, dependency resolution, signatures, and trust policy.
- Game-directory transaction and recovery implementation on supported Windows
  filesystems.
- Mad Max archive formats, repacking constraints, and game-build detection.
- Cache location, quota/cleanup policy, and user-visible storage controls.
- Whether local extraction/editing tools ship as part of Vapor Client or as a
  separately versioned Mad Max adapter launched and managed by Vapor.
- Which Mad Max operations require the executable to be closed and how the
  launcher enforces that.

## First implementation slice after approval

The first implementation should establish boundaries and a safe vertical
slice, not attempt every asset editor:

1. Add a provider-neutral Mad Max ecosystem/composition contract without
   changing unrelated service ownership.
2. Define the local installation record, baseline manifest, and rebuildable
   cache metadata.
3. Implement read-only game detection/indexing before any live-file mutation.
4. Specify a local transaction journal and recovery state machine.
5. Add a dry-run composition resolver and change summary; only then enable
   apply/rollback for a narrowly supported file operation.

The first slice should not launch or modify a game installation until the
preflight and recovery behavior is reviewable.

## Approval boundary

Approval of this design authorizes writing a detailed implementation plan. It
does not authorize deployment, game-file mutation, distribution of game-derived
content, or changes to Steam/Loo-Cast behavior beyond the explicitly scoped
provider-independence work.
