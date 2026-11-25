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
GET /api/updates/v1/vapor/stable/windows-x86_64/versions.json
```

The exact immutable release endpoints are:

```text
GET /api/updates/v1/vapor/releases/{version}/{platform}/manifest.json
GET /api/updates/v1/vapor/releases/{version}/{platform}/manifest.json.sig
GET /api/updates/v1/vapor/releases/{version}/{platform}/vapor.tar.gz
GET /api/updates/v1/vapor/releases/{version}/windows-x86_64/vapor-setup.exe
GET /api/updates/v1/vapor/releases/{version}/linux-x86_64/vapor-setup
```

The channel's `versions.json` endpoint returns the complete, newest-first list
of stable semantic versions that have a complete immutable release for that
platform. It is derived from the immutable release directories; advancing a
stable channel pointer does not require a separate history-file update. Each
listed version still uses its own immutable manifest and detached signature;
the endpoint is a discovery index, not a replacement signature. The Installer
continues to verify the exact manifest bytes before installing. The Homepage
keeps the channel's latest version as its primary download and renders older
entries as stable-release history.

The latest JSON response is served byte-for-byte from the selected immutable
`manifest.json`; its `.sig` response is served byte-for-byte from the matching
immutable `.sig`. The signature is lowercase hexadecimal Ed25519 over the exact
manifest response bytes. The client release contract requires checking that
signature and the artifact size and SHA-256 before staging an update.

Manifest fields are `schema` (integer `1`), `key_id`, SemVer `version`,
`channel`, `platform`, `minimum_updater_version`, `artifact` (`url`, `size`,
`sha256`), `setup` (`url`, `size`, `sha256`), and `release_notes_url`. The
signed setup record identifies the website-distributed `vapor-setup` binary;
that binary installs only the signed application archive. The manifest is not rewritten or
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

The current Vapor-Client feature branch contains the standalone Windows/Linux
GUI bootstrap and CLI lifecycle. The Installer pins the production Ed25519
public key, verifies the exact signed manifest, checks archive size and
SHA-256, downloads over HTTPS, rejects unsafe archive entries, stages immutable
versions, and atomically activates or repairs them. The `vapor-setup` binary
opens the GUI without arguments and accepts `install [--channel stable]` for
headless automation through the same signed acquisition path. The GUI
workspace compiled through Vapor-managed Linux and Windows release builds; the
staged Linux binary's help output includes the headless command. Vapor-managed
Linux and Windows application archives have been assembled and signed locally,
and the publish script accepted both against isolated temporary storage. No
signed release is published on Artifact yet, so the live feed and clean-machine
bootstrap have not been exercised. The Client now contains a default-branch
release workflow and restricted publication receiver/provisioning scripts;
the first live seed release and server/GitHub credential setup remain pending.

## Restricted CI publication

The default-branch Vapor release workflow streams one prepared bundle over SSH
to `deploy/scripts/vapor-release-publish-receiver.sh`. It does not use an
upload API, SCP, a general-purpose deploy key, or a shell account with arbitrary
commands. The receiver accepts only `publish linux-x86_64` or
`publish windows-x86_64`, rejects links, special files, unexpected paths,
duplicate entries, and incomplete bundles, then delegates signature and
artifact validation to the existing publisher.

Provision the dedicated account and forced-command key after deploying the
receiver and publisher scripts:

```text
sudo bash deploy/scripts/install-vapor-release-publisher.sh publisher_ed25519.pub vapor-release-public.pem
```

The first argument is a newly generated Ed25519 SSH public key whose private
half is stored only as the protected `VAPOR_RELEASE_SSH_PRIVATE_KEY` GitHub
environment secret. The second is the Ed25519 release verification public key
matching the key pinned in Vapor Installer. The installer creates
`vapor-release-publisher`, installs a forced-command key, and grants that user
sudo permission for only the root-owned receiver. Do not reuse an operator or
deployment SSH key.

Configure the `vapor-stable-release` GitHub environment with the signing key
secret `VAPOR_RELEASE_SIGNING_KEY_B64`, publisher key secret
`VAPOR_RELEASE_SSH_PRIVATE_KEY`, pinned host key secret
`VAPOR_RELEASE_SSH_KNOWN_HOSTS`, and variables `VAPOR_RELEASE_SSH_HOST` and
`VAPOR_RELEASE_SSH_USER` (`vapor-release-publisher`). The environment should
allow the `main` branch. Keep the signer key protected from pull-request jobs;
the workflow publishes only after its Vapor build, staging, archive validation,
and signing steps succeed on `main`.

The workflow does not use GitHub checkout for project sources. It installs the
current signed Vapor stable release, provisions Rust/Cargo and cross-build
tools in Vapor User Data, and asks Vapor Installer to clone the Client at the
triggering commit and Platform at a resolved commit under `~/Vapor/sources`.
Cargo build, staging, and output-root discovery go through Vapor.

## Operator-managed storage

`VAPOR_UPDATES_ROOT` defaults to `/var/lib/vapor-server/updates`. No upload or
publish route exists. The operator script stages the verified, signed bundle
into this tree and atomically updates a channel pointer:

```text
channels/{channel}/{platform}.version
releases/{version}/{platform}/manifest.json
releases/{version}/{platform}/manifest.json.sig
releases/{version}/{platform}/vapor.tar.gz
releases/{version}/{platform}/vapor-setup[.exe]
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
