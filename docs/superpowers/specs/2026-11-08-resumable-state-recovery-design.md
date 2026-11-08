# Resumable state-only fresh-host recovery

## Intent

Make an interrupted clean-host rebuild recoverable without asking the owner to
reinstall the VPS or guess which setup steps completed. The owner-held USB
continues to provide a verified state archive and a source-pinned recovery kit;
host packages, service configuration, and provider credentials are recreated
on the host.

## Current behavior

`recover-state-only-host.sh` validates the archive before changing the host,
prompts for host/provider settings, installs Ubuntu packages, checks out the
archive's Platform revision, deploys it, imports state, and checks provider
readiness. It is currently one-shot: an interrupted run leaves files behind,
but there is no operation record that explains whether the archive, source,
configuration, or deployment is the expected one. The README directs the owner
to inspect or reinstall the host before retrying.

The older full-host recovery script already has a useful owner-confirmed resume
flow. This change brings the state-only path to that operational standard while
keeping the state-only trust boundary: no host configuration or credentials are
copied from USB.

## Approaches considered

1. Keep the one-shot script and require manual host cleanup. Rejected because a
   routine interruption turns recovery into another manual VPS rebuild.
2. Resume from an unstructured presence check of files. Rejected because stale
   or unrelated host paths could be mistaken for progress.
3. **Use a root-owned marker pinned to the archive digest and source revision,
   validate every resume, and make recovery phases explicit.** Selected because
   it matches the existing full-host recovery operator flow and prevents a
   retry from silently switching archives or source.

## Design

- First-run preflight remains read-only until archive checksum, archive schema,
  source revision, Ubuntu architecture, and existing-host path checks pass.
- A separate marker under `/var/lib/vapor-state-recovery/` records only the bundle SHA-256,
  pinned Platform commit, and phase (`building` or
  `awaiting_provider_checks`). It is root-owned, mode `0600`, in a root-owned
  mode `0700` directory, and replaced atomically.
- `--resume` is accepted only with an existing marker whose schema, digest,
  revision, phase, ownership, and permissions match the supplied archive. A
  markerless non-empty host remains a hard stop.
- Resume reuses valid root-owned host configuration if bootstrap already wrote
  it. If setup stopped before configuration was created, the owner is prompted
  again. Existing source paths are reused only when the repository identity,
  exact pinned commit, submodules, and clean worktree validate.
- Failure after host mutation stops/disables partial Vapor services and keeps
  the marker and verified checkout for a retry. It never deletes unrelated
  host paths or replaces an unexpected marker.
- After state import, recovery requires configured provider readiness and
  leaves an `awaiting_provider_checks` marker. The owner completes browser
  sign-in checks and then reruns `--resume --confirm-provider-checks`; only then
  is the marker removed and recovery reported complete.
- The state archive remains the only USB data input. Resume markers contain no
  secrets. Credentials remain only in root-owned `/etc/vapor-server` config.
- The existing `recover-fresh-host.sh` flow is unchanged; this is parity for
  the state-only recovery entry point, not a new recovery authority.

## Verification

- Unit tests cover strict marker validation, atomic phase updates, ownership and
  permission rejection, digest/revision mismatch, and completion cleanup.
- Contract tests verify the state-only script exposes the resume contract,
  retains archive-before-mutation ordering, validates pinned source paths, and
  does not import secrets/configuration from the archive.
- CI runs shell syntax checks and the recovery test set. The live VPS is not a
  test host for this feature; a disposable fresh Ubuntu host is still required
  for end-to-end rebuild proof.

## Risks and limits

- A hard power loss during the transactional state import is recoverable by
  rerunning the verified restore against the same archive; the restore script
  retains its own pre-restore copy and rollback behavior.
- Credentials must be re-entered if interruption happens before bootstrap
  persisted them. After that, resume reuses the root-owned configuration.
- This does not prove provider integrations are live until the owner completes
  the final browser checks.
