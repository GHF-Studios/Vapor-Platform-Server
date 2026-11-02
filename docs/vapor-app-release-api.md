# Vapor application release API

This domain publishes Vapor application releases. It is separate from the
Vapor content registry and from Mad Max mod packages. The server stores no game
files; a release archive contains only the Vapor application and its own
resources.

`Vapor-Artifact-Server` owns the read routes and immutable release storage. The
Platform Server composes that service router into the one-process deployment.
Offline release preparation and the operator-only promotion script remain in
the Platform Server release tooling. No public upload route exists.

## Public read contract

The stable Windows feed is:

```text
GET /api/updates/v1/vapor/stable/windows-x86_64/latest.json
GET /api/updates/v1/vapor/stable/windows-x86_64/latest.json.sig
```

The exact immutable release endpoints are:

```text
GET /api/updates/v1/vapor/releases/{version}/{platform}/manifest.json
GET /api/updates/v1/vapor/releases/{version}/{platform}/manifest.json.sig
GET /api/updates/v1/vapor/releases/{version}/{platform}/vapor.tar.gz
```

The latest JSON response is served byte-for-byte from the selected immutable
`manifest.json`; its `.sig` response is served byte-for-byte from the matching
immutable `.sig`. The signature is lowercase hexadecimal Ed25519 over the exact
manifest response bytes. The client release contract requires checking that
signature and the artifact size and SHA-256 before staging an update.

Manifest fields are `schema` (integer `1`), `key_id`, SemVer `version`,
`channel`, `platform`, `minimum_updater_version`, `artifact` (`url`, `size`,
`sha256`), and `release_notes_url`. The manifest is not rewritten or
re-serialized by the server.

## Preparing and publishing a release

`deploy/scripts/prepare-vapor-release-bundle.sh` runs on the trusted release
operator's machine. It accepts the finished application archive and an Ed25519
private key, computes the archive hash and size, creates the exact manifest,
signs those bytes, verifies the signature locally, and writes a bundle matching
the server's directory layout. The private key is never copied to the output.
It requires Bash, Python 3, OpenSSL with Ed25519 support, and standard Unix
file utilities. The client currently trusts the fixed `vapor.ghf-studios.site`
origin, so bundle preparation rejects a different public origin. It also
rejects unsafe archive paths, links and special files, oversized packages, and
obvious Mad Max executable/install-directory entries before signing.

The completed bundle is transferred to the VPS. On the VPS,
`deploy/scripts/publish-vapor-release-bundle.sh` accepts the bundle, matching
public key, and expected client key id. It checks the manifest, artifact
hash/size, key id, and signature, then atomically installs the immutable version
and updates the channel pointer last. It refuses to overwrite an existing
version. The public key is safe to store on the VPS; the private signing key is
not.

Example operator flow:

```text
bash deploy/scripts/prepare-vapor-release-bundle.sh vapor.tar.gz 0.5.0 stable windows-x86_64 0.5.0 vapor-release-2026 private-ed25519.pem /tmp/vapor-release-0.5.0
# Transfer that prepared folder to the VPS through your approved secure method.
VAPOR_UPDATES_ROOT=/var/lib/vapor-server/updates bash deploy/scripts/publish-vapor-release-bundle.sh /path/to/vapor-release-0.5.0 public-ed25519.pem vapor-release-2026
```

Use a private-key file kept outside the repository and VPS. The key id must
match a public key trusted by the client build. Release preparation does not
build Vapor; it packages a previously built archive, which must contain Vapor
only and no Mad Max game files or extracted game content.

This feed and archive pipeline is not yet a Windows GUI installer or a complete
self-update installation flow. Vapor-Installer now contains a client-side
Ed25519 manifest verifier and streaming archive size/SHA-256 verifier. They are
not yet connected to network downloads, safe extraction, staged application,
rollback, or GUI update status, and no production public trust key is embedded.
Those remain client work before a public release is usable end to end.

## Operator-managed storage

`VAPOR_UPDATES_ROOT` defaults to `/var/lib/vapor-server/updates`. No upload or
publish route exists. The operator script stages the verified, signed bundle
into this tree and atomically updates a channel pointer:

```text
channels/{channel}/{platform}.version
releases/{version}/{platform}/manifest.json
releases/{version}/{platform}/manifest.json.sig
releases/{version}/{platform}/vapor.tar.gz
```

The pointer contains only the selected version and a trailing newline is
allowed. Latest requests return 404 until all pointed-to release files exist.
Latest responses have short revalidation caching; immutable release files use
one-year immutable caching. Route path segments are restricted to safe ASCII
characters and canonical paths must remain beneath the configured release
root.

The server streams application archives and serves public reads only. Private
signing keys must stay in the release pipeline/offline operator environment,
never in this repository or on the public host. A Mad Max game installation is
acquired and managed locally by the client; it is never bundled into Vapor
application releases.

See `vapor-standalone-install-and-source-home-contract.md` for the local data
locations that the client must preserve across update and uninstall, and for
the lazy checkout behavior expected when a user installs developer tooling.
