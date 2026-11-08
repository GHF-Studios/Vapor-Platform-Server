# Owner USB Recovery Status

## Intent

Show whether the owner-held USB recovery copy was recently synchronized from
the private Platform Recovery page. This makes backup freshness actionable
without implying that the VPS can inspect or independently prove the USB.

## Goals

- After the scheduled USB task verifies the indexed archives and recovery kit,
  it may report a small summary through its existing root SSH connection.
- Platform stores a bounded, sanitized report and exposes it only through the
  existing root-authorized recovery status API.
- The Recovery page distinguishes a recent owner-reported sync, a stale report,
  and no valid report. It also displays the reported archive digest and the
  report's verification counts.
- Failed status reporting must not invalidate a successful local USB copy.

## Non-goals and anti-goals

- The VPS does not read USB contents or claim that a report proves possession.
- No web upload route, new credential, public endpoint, or new authority is
  introduced. The existing owner root SSH identity is used.
- No drive letter, user profile path, private key, archive content, or secret is
  sent to the VPS.
- A report does not certify that a clean-host restore succeeded.

## Approaches considered

1. **VPS polling the USB:** impossible while the USB is local and disconnected.
2. **Browser upload:** adds a web mutation route and another authorization and
   request lifecycle for a report the scheduled task already produces.
3. **Owner task reports over its existing root SSH channel (selected):** reuses
   the authenticated path already used to fetch and verify backup archives;
   adds no credential or web mutation surface. The record remains explicitly
   client-reported evidence.

## Data and ownership

The Windows sync task owns the claim that it checked local archive bytes
against the VPS index. It submits only the latest archive name and digest,
last-success UTC time, verified/unverified archive counts, retained payload
bytes, and recovery-kit validation/source-commit facts. A fixed root-owned
Platform helper validates the bounded payload and atomically stores the
allowlisted summary under the backup root. Platform's existing recovery-status
helper reads the record and returns a typed projection; the browser never reads
the file directly.

Status is `recent` for 26 hours after the reported success, `stale` afterward,
and `unknown` when no valid report exists. The UI must call this
“owner-reported” status and state that the VPS cannot independently verify USB
possession. The record is local operational evidence, not an audit log.

## Failure behavior

If SSH reporting fails, local sync remains successful and its USB report records
that the platform report did not complete. Invalid or oversized remote reports
are rejected without replacing the previous good record. A missing or invalid
stored record appears as unknown and does not break other recovery evidence.

## Verification

Tests cover payload validation, path/schema limits, atomic replacement,
root-status projection and freshness, no-report/failure UI states, and PowerShell
sync behavior when report transport succeeds or fails. The production check
confirms the root-authorized page renders the report as self-reported evidence.
