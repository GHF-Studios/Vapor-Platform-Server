# Platform Recovery Workbench Design

## Problem

The shared admin workbench has deployment and service pages, but no dedicated
Recovery destination. The owner therefore cannot see recovery freshness and
limits from the web interface, even though a fresh-host rebuild is a primary
Platform responsibility. The VPS archive and the owner's USB copy are distinct
evidence; neither one proves a successful restore drill.

## Desired result

Add a root-only Recovery page that summarizes the latest VPS recovery archive,
the scheduled export timer, and what is still unverified. It must clearly
distinguish “an archive exists” from “the platform is recoverable on a fresh
host.” It does not launch restore, export, wipe, or shell operations.

## Approaches considered

1. **Read the backup directory from the web process.** Rejected: the directory
   is root-only and broadening its permissions would expose recovery material.
2. **Have the web process request one fixed status operation from the existing
   root helper.** Selected: the helper returns a small, bounded, non-secret
   summary for exact allowlisted inputs. The web route requires a root Identity
   session, invokes only the fixed subcommand, validates its response, and
   returns no-store JSON.
3. **Write a status file during each export.** Rejected for the first slice:
   a failed export could leave a misleading old status unless a second failure
   reporting path were added, and timer state would still be separate.

## Status semantics

- VPS archive recency is based on the newest matching regular archive's file
  modification time; “recent” means no older than two hours, matching the
  hourly export schedule with one missed run of tolerance. A timestamp more
  than five minutes in the future is “unknown,” never recent.
- The UI says “Recent archive found — validation not verified.” Neither a
  filename, size, sidecar, nor timestamp proves archive integrity or a
  successful restore.
- The UI may report archive count, size, pinned short commit, and whether the
  expected checksum sidecar exists. Sidecar presence does not claim that the
  archive has just been rehashed or restored.
- Timer activity and its next scheduled time are shown as bounded display
  strings reported by systemd. Missing or unreadable state is “Unknown” or
  “Unavailable,” never “healthy.”
- The latest state-export service outcome is shown independently from archive
  freshness. `running`, `failed`, `never run`, and `unknown` must not be
  presented as a recent successful snapshot. A completed successful run may
  show its bounded systemd start timestamp; archive freshness continues to be
  derived only from the published archive mtime.
- Archive-root state distinguishes a readable folder, a missing folder, and an
  unavailable folder. A permission or I/O error must not be presented as “no
  archive found.”
- Owner USB synchronization is reported as “Not reported to Platform” until a
  separately designed authenticated local sync report exists.
- Fresh-host restore is reported as “Not recorded” until a disposable-host
  drill produces explicit evidence. The dashboard cannot infer it from archive
  generation.

## Boundaries and safety

- Platform owns host recovery status and the shared page frame. Service-owned
  data and authorization remain with their services.
- The root helper accepts only the exact `recovery-status` subcommand and no
  user parameters. It reads only fixed paths and emits a bounded schema; it
  never returns archive contents, environment values, credentials, or raw host
  paths.
- The route checks root authorization before invoking the helper, uses a short
  timeout, rejects oversized or malformed output, and marks responses
  `private, no-store`.
- This slice is read-only. Recovery execution remains in the reviewed
  command-line fresh-host flow until a separate restore-operation design and a
  disposable-host drill establish safe UI semantics.
- The dashboard is internet-reachable even though it requires root Identity
  authorization. A future browser terminal must be a separate high-risk
  capability with explicit fresh reauthentication, a short session grant,
  session revocation/expiry, strict audit, and isolation from ordinary admin
  routes. The Recovery page does not provide or imply terminal access.

## Acceptance

- The admin navigation has one Recovery destination with clearly separated
  VPS archive, timer, owner USB, and fresh-host drill statuses.
- A missing helper, inactive timer, missing archive, malformed response, or
  stale archive cannot render as ready.
- A failed or in-progress latest export attempt remains visible even if an
  older archive is still recent.
- Non-root and unauthenticated requests cannot invoke the privileged helper.
- The deployed source remains independently testable in both supported
  platform paths; no direct production restore or destructive action is added.
