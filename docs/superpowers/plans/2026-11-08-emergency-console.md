# Emergency Browser Console Implementation Plan

> Implement the approved boundary in `docs/superpowers/specs/2026-11-08-emergency-console-design.md`.

## Scope

- Add Platform-owned grant/audit state and root-only arm, stop, status, and
  Caddy forward-auth routes.
- Add a focused Emergency Console page beside Recovery.
- Add a dedicated `ttyd` systemd unit, root helper allowlist, package installer,
  and tightly scoped Caddy route over a private Unix socket.
- Keep the console opt-in by root-owned configuration; do not deploy or enable
  it on the live VPS without a separate deployment review.

## Invariants

- Platform web process remains unprivileged; only exact `emergency-console-start`
  and `emergency-console-stop` sudo commands are allowed.
- Root Identity authorization completes before DB mutation or helper calls.
- Arm/stop are same-origin JSON requests with bounded schemas and
  `private, no-store` responses.
- A single 20-minute session globally; opaque cookie is scoped to the terminal
  path, stored only as a hash, bound to profile and Identity session.
- Every terminal upstream request passes Caddy forward-auth; only the fixed
  terminal service Unix socket is reachable by Caddy.
- The forward-auth callback itself is not publicly routable; the shared Caddy
  gateway returns 404 for direct public requests to that exact API path.
- ttyd receives a fixed wrapper command, no URL arguments, no host selection,
  one WebSocket client, same-origin check, one-shot server, and no transcript
  persistence. Bash has a prompt idle timeout; systemd enforces the hard limit.
- Existing deployment approval and Recovery behavior remain separate.

## Work sequence

1. Add bounded session/audit schema and lifecycle helpers to Platform.
2. Add grant and forward-auth endpoints, including session binding, cookie
   handling, origin checks, capability gating, and exact subprocess calls.
3. Add the Emergency Console page and accessible start/end confirmation flow.
4. Add the root helper's fixed start/stop actions and sudoers entries.
5. Add the `ttyd` systemd service and wrapper, disabled-by-default installer,
   and Caddy socket route.
6. Review grant transitions, cookie scope, Caddy proxy headers, service
   isolation, concurrent disable/start races, durable expiry events, and
   failure handling; compile and check shell/config syntax where local tools
   permit.
7. Push the feature branch, inspect CI, and do not deploy or enable the console
   without the owner's explicit deployment request.

## Review checklist

- Authentication precedes all side effects.
- Revoke wins over concurrent authorization; stopped backend leaves no valid
  grant.
- Cookie never appears in HTML, URLs, logs, JSON, or audit data.
- Ttyd is not reachable by TCP and Caddy is its only socket peer.
- User-supplied values never become command arguments or Caddy upstreams.
- Auth failure, helper timeout, ttyd absence, or stop failure fail closed.
- Installer accepts only an exact reviewed ttyd version; no version is
  currently approved, so preparation remains blocked until upstream fixes the
  reported crash and that release is reviewed.
- The UI states the missing recent-authentication/second-factor limitation.
- The feature remains disabled until the shared-origin XSS risk is addressed by
  an isolated admin origin/session boundary or a verified Identity step-up.
