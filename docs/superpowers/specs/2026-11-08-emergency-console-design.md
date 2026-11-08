# Emergency Browser Console Design

## Intent

Provide a root-only browser terminal inside the separate Emergency area. The
dashboard is internet reachable, so this feature is a deliberate root-shell
capability exposed through HTTPS. It is not a replacement for Strato's VNC
recovery path or OpenSSH.

## Approaches considered

1. **Spawn a shell from the Platform web process.** Rejected: this would put a
   root shell behind the general Platform request process and expand its
   privilege boundary.
2. **Run a dedicated terminal transport behind Caddy and Platform auth.**
   Selected: `ttyd` owns only the PTY/WebSocket, runs as its own one-client
   systemd unit, and listens on a private Unix socket. Caddy is the only
   socket peer. Caddy calls a fixed Platform authorization endpoint before
   forwarding every terminal page, asset, and WebSocket request. Platform
   checks the root Identity session and a short-lived console grant, then
   returns a header Caddy copies to ttyd. The browser never receives that
   header or a server credential.
3. **Expose an SSH/WebSocket gateway or Cockpit instance.** Rejected for this
   slice: it adds a second login/host-selection surface or requires shipping
   SSH credentials to another service, while the desired target is this VPS
   only.

## Authorization and session lifecycle

- Opening the Recovery page grants no terminal access.
- The owner explicitly starts one console session from the Emergency Console
  page after acknowledging that this is a root shell.
- Platform verifies the current root Identity session and same-origin POST,
  creates one global session record, and issues an opaque `Secure`, `HttpOnly`,
  `SameSite=Strict` cookie scoped only to `/emergency-terminal/`.
- The database stores only a hash of that cookie, the Identity profile id, a
  hash of the Identity session token, timestamps, connection state, and
  lifecycle status. It never stores terminal input or output.
- The grant expires no later than 20 minutes after creation. Caddy also closes
  the WebSocket after 20 minutes. The dedicated systemd unit has a 20-minute
  hard runtime limit, one client, and exits after disconnect. Bash also closes
  after ten idle minutes at an interactive prompt. A new shell requires a new
  explicit grant.
- Ending the session revokes the grant first, then asks the fixed root helper
  to stop the dedicated unit. If stopping fails, the grant still rejects new
  connections and the unit's hard runtime limit remains in force.
- The terminal WebSocket must present the configured HTTPS origin. Caddy
  forwards to ttyd over a root-owned socket readable only by the `caddy` group;
  ttyd is not bound to a TCP port. URL command arguments, tunnels, file
  transfer, and host selection are disabled.
- The shell starts with history disabled. Vapor and Caddy do not retain a
  terminal transcript. The page warns that commands and displayed secrets may
  still exist in the user's browser memory or clipboard.

## Security boundary and explicit limits

- This capability is equivalent to root SSH. A root Identity session plus a
  separate explicit grant is the current step-up boundary; Identity does not
  currently expose a verified recent-password/WebAuthn claim to Platform.
- Therefore the UI must say that the grant is explicit and short-lived but is
  not a second factor. Never describe this as stronger than the Identity
  session that authorizes it.
- Terminal transport is isolated from the Platform web process, but a
  vulnerability in ttyd, Caddy, Identity, or the browser route can still expose
  root access. The deployment has a global enable/disable switch and the
  feature remains disabled unless the owner enables it in root-owned config.
- Upstream currently reports a zero-length WebSocket crash for ttyd 1.7.7 and
  has an open proposed fix. Here, forward-auth means reaching that frame handler
  already requires a root Identity session and active console grant; the
  reported impact is terminal availability, not unauthenticated access. Recheck
  upstream status before enabling this for more than the owner account.
- The forward-auth callback is a loopback-only internal interface; Caddy must
  return 404 for public requests to that callback before the general admin API
  path proxy. Caddy overwrites the internal context headers when proxying the
  terminal path.
- A global disable transition revokes the active grant and stops the terminal
  unit. Host-level emergency recovery remains Strato VNC or SSH.
- Access records keep actor/profile, start, expiry, end/revoke reason, and
  backend lifecycle result. They do not record commands, environment values,
  or terminal output.

## Non-goals

- No private SSH key is copied to the VPS web process or browser.
- No generic remote-host SSH client, port forwarding, file transfer, or
  command-history service.
- No automated platform wipe/rebuild is exposed from the terminal page.
- No claim of fresh Identity reauthentication or second-factor protection.

## Acceptance

- Without root Identity authorization, no page action, auth callback, or
  terminal resource is available.
- A root session alone cannot connect: a separate active console grant is
  required.
- A forged Origin, wrong path, expired/revoked grant, different Identity
  session, or missing cookie cannot reach the terminal upstream.
- Only one console grant and one WebSocket client can be active at a time.
- Grant creation and termination are recorded without transcript content.
- Expiry, explicit stop, and global disable revoke server authorization and
  stop the dedicated terminal service where possible.
- Recovery remains a separate page and records-only surface.

## References

- Caddy `forward_auth` and WebSocket proxy behavior:
  https://caddyserver.com/docs/caddyfile/directives/forward_auth
  https://caddyserver.com/docs/caddyfile/directives/reverse_proxy
- ttyd's documented Unix socket, auth-header, origin, base-path, one-client,
  and one-shot options:
  https://github.com/tsl0922/ttyd
- ttyd currently has no published GitHub security policy or advisories; this
  is not treated as proof of security:
  https://github.com/tsl0922/ttyd/security
- Upstream ttyd 1.7.7 empty-frame crash report and proposed fix:
  https://github.com/tsl0922/ttyd/issues/1546
  https://github.com/tsl0922/ttyd/pull/1574
