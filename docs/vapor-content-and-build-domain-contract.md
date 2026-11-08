# Vapor content and build domains

Status: canonical boundary contract for Registry, distribution, Build Snapshots,
shared compiler caching, and native build execution. This describes the intended
system and calls out the parts that are not implemented. It does not make a
future capability appear available merely because a service or admin tab exists.

## The four different things called “content” or “build output”

Vapor needs distinct records and lifecycles for these concepts. They may refer to
one another by stable IDs and digests, but one record must not impersonate
another.

| Concept | Canonical meaning | Owner |
| --- | --- | --- |
| Ecosystem source topology | Repositories and checkout requirements used to develop or reconstruct an ecosystem | Registry (current implementation) |
| Vapor content identity | A named, typed, versioned thing that can be installed, mounted, composed, or referenced | Registry (target; not implemented) |
| Distribution/acquisition binding | Where a particular release is obtained: Vapor Artifact, Steam Workshop, a locally installed Steam depot, or another declared source | Registry records the binding and policy; the provider remains authoritative for acquisition |
| Product composition | A compatible set of content releases and locally acquired prerequisites that form a runnable product | Registry (target; not implemented) |
| Application release | A signed Vapor executable/package release | Artifact Server |
| Build snapshot | A signed Cargo target directory snapshot restored to accelerate a developer workspace | Build Cache service (currently implemented as snapshot storage/validation/promotion) |
| Shared compiler cache | Reusable compiler results keyed by the full toolchain/target/flags/environment identity (for example, `sccache`) | Not selected or implemented |
| Native build operation | A requested build with pinned inputs, executor assignment, progress, logs, cancellation, result, and cleanup | Platform orchestration (candidate path not implemented) |

“Build Cache” is the service/repository name. Its current stored object is a
**build snapshot**, not a cache of independent compiler invocations. The current
service can accept and promote a correctly signed snapshot candidate, but there
is no production native builder submitting candidates, and Platform has no
durable native-build job lifecycle yet.

## Content identity is not a repository or a Workshop item

A content ID names a Vapor concept independently of its source repository or
distribution provider. A source repository may contain many content items; one
content release may be assembled from multiple sources; and a provider item may
carry a release without defining its Vapor type or permissions.

The target Registry model therefore separates:

1. **Content record:** stable namespaced ID, display metadata, explicit Vapor
   type, owner/author identity, visibility, and lifecycle state.
2. **Source references:** zero or more pinned repository/revision/subpath
   references. Source is optional for locally acquired proprietary content.
3. **Release:** immutable version identity plus a content digest and declared
   compatibility/dependency metadata.
4. **Acquisition binding:** provider and provider-native identifier for a
   particular release, with provenance and an explicit policy class such as
   public upload, first-party depot, Workshop, or external local source.
5. **Composition:** a named product set that references exact releases and
   prerequisites. Installation must resolve availability and compatibility
   before mutating the live game directory.

Steam Workshop is one possible distribution provider, not the content catalog
and not the source of Vapor's type system. A Workshop item must resolve to a
Vapor content ID and release before Vapor treats it as typed content. Steam
depot files are locally acquired prerequisites: Vapor may index and mount them,
but the server must never accept an upload or serve a download of those bytes.
First-party or licensed content must be authorized from server-owned policy,
not from a client-supplied manifest assertion.

For Mad Max, the intended mapping is expressed through typed content records:
the game executable is an external engine prerequisite; the mod adapter and
loader integration are package-pack content; mod scripts and mounted additions
are game/mod content. Loo Cast can ship its first-party engine, game, and
package-pack together through its own Steam depot and map those local files as
depot-provided content. These examples are mappings, not implemented Registry
records yet.

## Build lifecycle and authority

Build outputs have separate pipelines:

```text
Pinned source + Vapor/toolchain locks + target + recipe
                  │
                  ▼
       Platform build operation (not implemented)
       owns job ID, executor, state, logs, cancellation
                  │
        local executor or optional worker
                  │
                  ▼
     candidate snapshot ──► Build Cache validates and promotes
                                  │
                                  ▼
                      one current snapshot per target
```

- **Local source build:** must remain possible when no remote snapshot or worker
  is available. It builds source and does not publish an official snapshot.
- **Build operation:** Platform-owned durable lifecycle. A request pins source
  revision, all lock/recipe identities, target, resource limits, and executor
  policy. Every transition and terminal result is recorded by Platform; a
  current-pointer timestamp is not a job history.
- **Native builder:** an execution capability selected for a build operation,
  not a second cache or a product role. It reports bounded progress and returns
  a candidate. It receives no signing key or deployment authority.
- **Build Snapshot service:** validates signed candidate metadata and bytes,
  then atomically updates the current snapshot for one target. It owns snapshot
  retention/export and public retrieval. It does not choose or provision an
  executor.
- **Shared compiler cache:** optional optimization for compiler invocations,
  keyed by toolchain, target, build flags, wrapper configuration, and guest
  environment. It changes build time, never build correctness or whether local
  builds are allowed. It is not the restored Cargo target directory.
- **Application release:** goes through Artifact Server's signed release
  contract. A Cargo target snapshot is never a user-facing application release.

Current status is deliberately narrower: local Platform service deployment
builds on the primary VPS; Build Cache has signed snapshot read, validation, and
promotion code; no VPS/worker build orchestration, shared `sccache` service, or
durable native-build operation history is implemented. The optional second
worker and local fallback follow `vapor-build-snapshot-contract.md`; neither
requires a second public authority.

## Implementation order

1. Keep current Registry APIs explicitly named as ecosystem/source topology;
   do not label them a catalog.
2. Add Registry's content/release/acquisition schema and read API, keeping
   source topology as a separate relation. Write migrations before exposing
   writes; do not treat Workshop metadata as authoritative types.
3. Add provider/local-source policy checks and composition resolution before
   publishing or mutating installations. Identity remains the authorizer, and
   publish execution remains a separately owned operation.
4. Define Platform's durable build-operation record and executor interface.
   Show queued/running/failed/cancelled/published as operation states, not as
   Build Cache snapshot states.
5. Keep Build Cache candidate validation and atomic current-snapshot promotion
   independent. Add a shared compiler cache only as a separate optional lane
   after target identity and benchmark evidence exist.
6. Add native-builder executors and lifecycle/recovery only after operation
   records, worker permissions, cleanup, cost limits, and local fallback are
   explicit. A service-health check is not evidence that a builder exists.

## Non-goals and invariants

- No game, depot, or Workshop payload is uploaded to Vapor merely because it
  appears in a content manifest.
- No browser or client can make itself an author or first-party publisher by
  declaring that status in submitted metadata.
- No compiler cache can replace signed application releases or signed target
  snapshots.
- No worker owns public authority, Identity policy, snapshot promotion policy,
  deployment credentials, or long-lived signing keys.
- No current admin tab, route, or `healthy` status implies that the represented
  catalog, compiler cache, or builder workflow is complete.
