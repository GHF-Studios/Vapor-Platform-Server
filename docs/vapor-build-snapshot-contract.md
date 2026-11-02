# Vapor Build Snapshot Contract

Status: implementation contract for the single-VPS builder and Build Cache
service.

## Authority and topology

- The one Vapor VPS orchestrates both target builders. It hosts an isolated
  Linux VM and an isolated Windows VM, using the same provision/start/build/
  collect/clean lifecycle.
- VM image identity, guest build-environment identity, and the canonical build
  recipe are pinned inputs governed by the one Vapor toolchain lock.
- If the VPS cannot host both VMs with adequate virtualization support,
  resources, and Windows licensing, the affected target is unavailable until
  the VPS is upgraded or replaced. No external worker or owner PC is a fallback.
- The Build Cache service owns current snapshot manifests and archives. Platform
  Server owns orchestration and route composition, not cache policy or files.
- The VPS is the only publisher. A VM can submit only the candidate named by
  its short-lived, target-scoped build operation; it cannot select or replace a
  current snapshot directly.

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
- A missing or stale snapshot is an explicit setup failure with a repair/retry
  action. Client-side cold compilation is not a silent fallback.
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
