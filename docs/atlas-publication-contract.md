# Atlas publication contract

Atlas is maintained in `GHF-Studios/Atlas`. Platform Server checks it out as a
source submodule and publishes its generated static site at `/atlas/`.

- `VAPOR_ATLAS_REF` selects the source branch; it defaults to `main`.
- The normal Platform Server deployment poll fetches the selected ref. No Atlas
  SSH credential or Docs upload token is needed.
- Publication validates Atlas records before building. A failed validation or
  build leaves the currently served site unchanged.
- Builds are placed in a commit-named release directory under
  `/srv/vapor-web/atlas/releases/`; `current` is switched atomically only after
  a successful build.
- The current and immediately previous build are retained. Older generated
  Atlas output is removed after a successful switch.
- Caddy serves this directory directly. Atlas files never pass through the
  Docs service and cannot replace the `/docs/` artifact tree.
- The site is derived from source and is not included in mutable service-state
  backups. Re-deploying Atlas reconstructs it from the selected Git commit.
