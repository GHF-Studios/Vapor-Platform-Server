# Vapor Build Snapshot Contract

Status: target contract for an optional worker and the Build Cache service.
The worker path is not implemented or ready until the clean-worker exercise
below has passed.

## Authority and topology

- The primary Vapor VPS remains the sole public and policy authority. An
  optional second VPS may run isolated Linux and Windows target jobs and may
  replicate verified recovery archives. It does not host public routes or own
  service state.
- The second VPS is optional. If it is absent or unreachable, the supported
  local build path remains available, though slower. Worker outage may leave
  the public rolling snapshot stale; it must not block source builds, service
  deployment, or recovery of the primary.
- VM image identity, guest build-environment identity, and the canonical build
  recipe are pinned inputs governed by the one Vapor toolchain lock.
- The worker contract consumes the canonical lock from Vapor Client; this
  Platform repository must not invent a second lock. The recovery bundle still
  records observed host compiler versions for rebuild reproduction, which is
  not equivalent to the canonical product lock. Worker implementation stays
  blocked until the lock artifact and identity are available to the operation.
- Worker capacity, environment pinning, access, and Windows toolchain licensing
  must be proven before publishing snapshots. Until then, candidates can be
  built through the explicit local path; unvalidated output cannot replace the
  current snapshot.
- The Build Cache service owns current snapshot manifests and archives. Platform
  Server owns orchestration and route composition, not cache policy or files.
- Platform orchestration is the only publisher. A worker or local build can
  submit only the candidate named by its target-scoped build operation; it
  cannot select or replace a current snapshot directly.

## Supported target identities

The first supported targets, taken from the current Vapor app-source manifest,
are:

| Platform slug | Rust target | Guest |
| --- | --- | --- |
| `linux-x86_64` | `x86_64-unknown-linux-gnu` | Pinned Linux VM |
| `windows-x86_64` | `x86_64-pc-windows-gnullvm` | Pinned Windows VM |

The platform slug is presentation metadata. The Rust target triple is the
authoritative cache namespace. A target or guest-image change requires a new
Vapor/toolchain lock state; clients do not choose historical generations.

## Current snapshot API

Public reads are served beneath `/api/build-cache/`:

```text
GET /v1/targets/{target-triple}/current/manifest.json
GET /v1/targets/{target-triple}/current/manifest.json.sig
GET /v1/targets/{target-triple}/current/snapshot.tar.zst
```

Every request supplies the exact current client and lock identities:

```text
X-Vapor-Client-Version: <current rolling version>
X-Vapor-Lock-SHA256: <lowercase SHA-256 of canonical Vapor.lock bytes>
X-Vapor-Toolchain-Lock-SHA256: <lowercase SHA-256 of canonical toolchain-lock bytes>
```

The atomic `targets/{target}/current` pointer stores a schema version, the
current manifest digest, the operation ID that promoted it, and the promotion
Unix timestamp. This is the latest successful promotion record, not an
append-only audit log or a report of failed VM jobs.

The service returns `404` when that target has no current snapshot, `426 Upgrade
Required` when client or lock identity is stale, and `404` for unsupported
targets. Responses never reveal host paths, VM addresses, or secrets. The
current manifest is `no-store`; the archive may use immutable caching only when
addressed by its verified content digest.

Builder submission is internal and not exposed through the public route:

```text
POST /v1/internal/targets/{target-triple}/candidate
```

It requires a short-lived builder credential issued by VPS orchestration,
bound to the target, current lock identities, and one operation ID. The service
validates the candidate before atomically switching the `current` pointer. This
pointer records the manifest digest, operation ID, and promotion time in one
atomic file, providing the latest successful-promotion record with the current
snapshot identity. The previous current snapshot remains available until
validation and promotion have both succeeded.

## Signed manifest

`BuildSnapshotManifest` schema version 1 is UTF-8 JSON with these required
fields:

```text
schema_version                 integer, exactly 1
key_id                         trusted Build Cache signing-key identifier
vapor_version                  exact rolling Vapor version
vapor_lock_sha256               lowercase SHA-256 of canonical Vapor.lock bytes
toolchain_lock_sha256           lowercase SHA-256 of canonical toolchain-lock bytes
target_triple                   one of the exact supported Rust target triples
build_recipe_id                 canonical recipe identifier from Vapor.lock
builder_environment_digest      SHA-256 of the pinned guest image and build-environment descriptor
archive_sha256                  lowercase SHA-256 of the complete snapshot archive
archive_size                    exact archive size in bytes
created_at_utc                  RFC 3339 UTC timestamp
```

The detached signature is lowercase hexadecimal Ed25519 over the exact
`manifest.json` response bytes. The Build Cache signing key is dedicated to
snapshots and separate from the Vapor application-release signing key. The
private key is held only in protected VPS configuration, never in source,
archives, client packages, VM images, or the browser admin UI. VMs receive only
short-lived candidate-upload credentials. Client releases pin the public key
and key ID; rotation must overlap trusted public keys across a client update.

## Archive and restore rules

- The archive contains the Cargo target-root contents for exactly one Rust
  target triple, plus a small archive metadata file. It contains no source
  repositories, game files, credentials, or unrelated target outputs.
- Archive paths are relative, normalized, unique, and remain beneath the target
  root. Absolute paths, `..`, links, special files, duplicate paths, and
  unsupported entry types are rejected before extraction.
- The client verifies version/lock/target/environment identity, Ed25519
  signature, archive byte count, and SHA-256 before extracting into a temporary
  sibling directory. It atomically activates the verified tree only after
  extraction succeeds; failure leaves the prior target root intact.
- A missing or stale snapshot is shown explicitly. Developer workflows may use
  the supported local build path when no current snapshot is available; Player
  workflows that do not compile do not pretend local compilation is available.
  Snapshot absence is never silently reported as a cache hit.
- Promotion is atomic per target. At most one current snapshot per target is
  publicly addressable; old generations are not a client-selectable cache.
- Restore portability is not assumed from a matching triple alone. The VPS
  smoke build must restore the snapshot into a clean second home and prove the
  canonical managed build commands reuse it without a full workspace rebuild.

## Separation from other services

- Artifact Server remains the signed Vapor application release service.
- Registry remains the authority for canonical product and source policy.
- Content Upload remains separate from both application releases and build
  snapshots.
- Build Cache stores compiler output only; it never stores Mad Max or Loo Cast
  game/depot bytes.
- Whole-system export/import composes the Build Cache service's own export
  format. The cache service decides its retention and consistency rules.

## Optional ephemeral compile workers (queued)

The owner has proposed disposable native Linux workers billed by actual use,
backed by independent S3-compatible object storage and `sccache`. This is a
separate acceleration lane from the Build Cache snapshots above:

- `sccache` stores reusable compiler results; the Build Cache service stores
  validated, target-scoped developer snapshots for Vapor clients. Neither is
  a substitute for the other.
- `sccache` is not a remote copy of Cargo's incremental `target` directory.
  Rust incremental crates are not cached by default; a shared-cache worker
  recipe must either disable Cargo incremental compilation or deliberately
  leave those crates uncached. Local incremental state can still make repeated
  edits faster on one persistent machine.
- A worker should be created from a pinned image/recipe, receive only a
  target-scoped job, read and write a namespaced compilation cache, return a
  candidate, and then be destroyed. It must not own deployment authority,
  credentials for publishing, platform state, or proprietary game content.
- Cost comparisons should account for actual billed VM seconds, persistent
  object-storage capacity, cache requests, and data transfer separately. Prefer
  a provider whose compute billing stops at job completion and whose S3 API can
  be replaced without changing cache semantics; verify minimum billing periods,
  storage minimums, and egress charges before choosing one.
- The orchestration lifecycle is a target-scoped operation: queued, provisioning,
  compiling, candidate upload, validation/promotion, then worker teardown. A
  worker must be destroyed on success, failure, timeout, or cancellation; a
  janitor must reap instances whose controller disappeared. Report compute
  seconds, stored byte-hours (or the provider's equivalent billing unit),
  request counts, and egress independently so the owner can compare real cost.
- Give each worker only the short-lived read/write access needed for its
  namespaced compiler-cache prefix and candidate upload. Do not put long-lived
  object-store credentials or deployment credentials in the VM image. The
  Build Cache service still validates and promotes the candidate.
- Remote cache absence or outage must fall back to local native builds. Cache
  misses affect build time, not build correctness or recovery capability.
- Cache keys must include the locked toolchain, target triple, build flags,
  compiler-wrapper configuration, and guest environment identity. Do not treat
  a matching target triple alone as sufficient evidence of reuse.
- Do not select a provider or implement provisioning until the second-node
  capacity/cost and storage API constraints are known. Avoid tying cache
  semantics to any one S3 vendor; use the existing Build Cache service as the
  reviewed snapshot promotion boundary.
- Before provider selection, benchmark one representative Linux target with a
  cold compiler cache, a warm compiler cache, and a fully restored target
  snapshot. Record wall time, billed compute, cache bytes/requests, and egress;
  compare those results with the local fallback and the worker's idle/VM-image
  costs. The cache and worker remain optional unless those measurements show a
  practical improvement.

Cranelift and mold remain candidate measurements, not defaults. Production
artifacts continue using the canonical locked Rust/LLVM build recipe unless a
separate target contract proves another backend produces acceptable outputs.
