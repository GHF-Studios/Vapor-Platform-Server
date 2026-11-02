# Vapor standalone install and source-home contract

Vapor is distributed from `vapor.ghf-studios.site` independently of Steam and
Loo-Cast. The website distributes Vapor application releases only. It never
distributes Mad Max, another game, or a developer's editable source checkout.

## Local homes

Keep these locations distinct:

- **Vapor application home** owns the installed launcher/client, updater state,
  managed toolchains, logs, and application caches. The updater may replace
  application-owned files here.
- **Vapor Superworkspace** owns editable Git checkouts for ecosystem source
  repositories. Its default is the user's `~/Vapor` directory, and the user may
  configure a different location. The Superworkspace is not the application
  install directory and must not be removed by app updates or uninstallation.
- **Game installation and local source cache** contain files acquired from a
  user's own licensed installation. They remain local and are never included in
  a Vapor release or uploaded as part of source-repository acquisition.

The application home and Superworkspace may both be managed by Vapor, but they
have different lifecycles. In particular, an app installer must not place a
mutable source tree beside a protected application binary or scatter checkouts
beside the user's current project.

## Lazy source acquisition

Normal Vapor installation and normal game/content installation do not clone
Vapor ecosystem repositories. The explicit Ecosystem Developer role is the
first capability that should acquire the public Vapor development-source set;
Composer and Content Developer roles install their own tools without cloning
platform source. A later developer tool or developer-focused ecosystem package
may declare a narrower repository set by registry identity. When the user
chooses one of those developer capabilities, Vapor should:

1. Resolve the declared repository identities against the configured ecosystem
   registry.
2. Show the repository names and destination Superworkspace before acquisition.
3. Create or discover the Superworkspace, then clone only missing declared
   repositories into their registry-declared top-level checkout paths.
4. Validate any existing checkout's Git origin before reuse; never overwrite a
   mismatched directory.
5. Initialize declared Git submodules and report each acquired or reused
   checkout in the install result.

For the first role-triggered acquisition, Vapor should use the configured
`ghf-studios/vapor` ecosystem list. The install flow should initialize the
default Superworkspace (`%USERPROFILE%\Vapor` on Windows, `~/Vapor` on Unix)
if it has not yet been created, then pass the canonical configured root to the
existing acquisition action. It should report Git as a prerequisite if Git is
not present. Do not make every Developer role clone the platform sources.

The checkout request must use registry-authored repository metadata, not an
arbitrary URL or path supplied by an untrusted package manifest. Repository
checkouts are shared developer sources: uninstalling one tool must not delete a
checkout still used by another tool or opened by the user. Source cleanup should
be a separate, explicit action with a preview of what would be removed.

The public Vapor Registry must not list Loo-Cast as a provider repository or
ecosystem dependency. Installing Vapor or Vapor ecosystem developer tooling
must not require its checkout, account, or content.

Vapor already has explicit Superworkspace discovery and repository-acquisition
actions, but those actions are currently separate from developer-role promotion.
The remaining client integration is to initialize the managed Superworkspace
when needed and connect Ecosystem Developer promotion to repository acquisition.
Later, narrower developer packages can use the same acquisition machinery with
their own repository identities. This action must remain lazy and visible to the
user; it must not run as a side effect of downloading or updating Vapor.

## Update and install behavior

The public release feed supplies signed immutable manifests and application
archives. The client verifies the manifest signature and artifact hash before
staging an update. Updating Vapor must preserve the configured Superworkspace,
game installation, local source cache, and user content. Uninstalling Vapor
removes application-owned files only; it must not silently remove those user
data roots.

The client may offer an explicit cleanup screen later, but the first standalone
installer should prefer leaving user data intact and explain where it lives.

## Mad Max ecosystem boundary

Vapor can treat Mad Max as a locally mounted engine/game ecosystem. Setup may
locate the user's installation, validate it, and derive a metadata index or
cache locally. The application release and public server do not contain game
archives or extracted game assets. Mod profiles and developer tooling refer to
local mounted content without uploading proprietary game files.

## Resource access and author identity

Vapor downloads, updates, and explicitly public resources do not require a
Loo-Cast license. A resource marked protected by its owning catalog must be
authorized by that service before metadata, manifests, or artifacts are
returned. For the first Registry slice, Identity checks the current Vapor
session and, when the policy requires `loo-cast`, verifies its Steam license
server-side with the publisher-authorized key. The key stays in Identity; the
client and Registry never receive it. Mad Max remains a local-source
requirement only: the server does not query Steam for Mad Max ownership and
does not treat a client-reported local mount as remote license proof.

Future upload/publishing endpoints must derive the canonical creator from the
authenticated Identity profile at creation time, using a service-scoped opaque
creator subject rather than exposing internal profile IDs to clients. Any
author name supplied by a client is display metadata only. Update or delete
operations must authorize against the server-stored creator identity and the
service's policy. This server does not yet implement content uploads,
moderation, or owner-managed resource APIs.

## Release readiness checklist

- The release archive contains the Vapor application and its own resources only.
- A production update verification key is embedded in the client; private
  signing material remains outside this repository and the public server.
- The first-run flow offers a Superworkspace location and explains that source
  repositories are acquired on demand for developer tools.
- A developer tool install preview lists repository checkouts it will acquire.
- A normal player install does not require Git or acquire development sources.
- Uninstall and profile switching do not remove the Superworkspace or local game
  content.
- The website clearly distinguishes the Vapor download from the user's game
  installation.
