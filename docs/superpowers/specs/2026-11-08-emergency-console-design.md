# Emergency Browser Console Design

## Intent

Provide a browser-based emergency terminal reachable from the internet, but
only inside the root-only Emergency work area. The route being internet
reachable is accepted; anonymous or ordinary signed-in access is not.

## Security boundary

An interactive root shell over HTTPS is equivalent to remote root SSH. It is
not a harmless dashboard widget. A vulnerability in the web application,
Identity session, reverse proxy, or terminal transport could become host-wide
execution. The implementation must not place a root private key in the web
process or browser, and must not provide a generic shell through the existing
Platform process's broad privilege boundary.

## Required design before implementation

1. Isolate terminal transport in a dedicated service and systemd unit, bound
   to loopback and routed only through the shared gateway.
2. Require root Identity authorization plus a fresh, explicit emergency-session
   grant. A normal admin page visit must not start a shell. The grant should
   have a short hard expiry and idle expiry, be revocable, and be separately
   visible in audit history.
3. Bind the browser WebSocket to same-origin requests, the authenticated
   session, a per-session nonce, and strict origin checks. Reject replay,
   cross-site WebSocket hijacking, and arbitrary target host/port selection.
4. Disable SSH forwarding, tunnels, uploads, downloads, and arbitrary host
   selection. The service may connect only to the local VPS terminal backend.
5. Avoid retaining terminal input/output as an audit transcript because it
   may contain secrets. Record session identity, start/end, expiry/revocation,
   and transport failures without command contents.
6. Recovery controls remain separate. Any future export, import, reset, or
   nuke action needs its own explicit typed operation, confirmation, and
   recovery-point check; the terminal page does not replace those workflows.
7. Provide a global disable switch and document the recovery path through
   Strato VNC/SSH if the console service itself is unavailable.

## Open implementation choice

Choose and review an established PTY/WebSocket transport versus a small
Platform-owned transport. Do not write a terminal emulator or SSH protocol.
Evaluate dependency maintenance, privilege separation, session expiry/revoke,
and compatibility with the existing Caddy path-routing model before adoption.

## Explicit non-goals for the first recovery slice

- No arbitrary root shell endpoint is implemented by the Recovery page.
- No root SSH key is copied into a web process or exposed to the browser.
- No recovery archive contents or environment values are served over HTTP.
