# Temporary SSH Account Lock Sandbox Design

## Intent

Allow an approved temporary SSH grant to create its Unix account reliably while
keeping the long-running Platform service unprivileged and the operation
request-scoped. The live approval failed because `useradd` could not lock
`/etc/passwd`; the current template unit uses `ProtectSystem=strict` and only
allows writes to the account database files, not the sibling temporary and
lock files shadow-utils creates there.

## Approaches

- **Allow `/etc` in the request-scoped root operation unit (chosen):** keep
  `ProtectSystem=strict` everywhere else, but make `/etc` writable inside
  `vapor-platform-ops@.service`. This lets `useradd` perform its normal,
  synchronized account-database update, including dynamically named sibling
  files. The writable surface is broader within this short-lived helper.
- **Keep `/etc` restricted and replace `useradd`:** implement account database
  edits and locking ourselves. This duplicates shadow-utils behavior and risks
  breaking host account consistency, so it is rejected.
- **Move provisioning into another privileged service:** this adds another
  helper boundary without removing the requirement that the account mutator
  can create sibling files under `/etc`, so it adds complexity without
  shrinking the necessary write surface.

## Contract and scope

- Only the root-owned, one-request systemd helper instance gains `/etc` write
  access. The Platform web service and other long-running units do not change.
- The helper continues to accept only its bounded, enumerated request format;
  public-key, capability, approval, and expiry validation remain unchanged.
- Grant failure remains fail-closed: a grant is not reported active unless
  provisioning completes. Cleanup must leave no account, home, sudoers rule,
  or expiry timer when provisioning fails.
- Do not create persistent lock files or weaken host-level filesystem
  permissions to work around the service namespace.

## Verification

Inspect the unit and deployment packaging, run the repository's relevant
deployment validation, then confirm the installed unit and failed-request
cleanup on the live host before attempting a fresh grant. Do not run Cargo
builds or tests for this unit-only change.
