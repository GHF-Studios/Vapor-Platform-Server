# Admin Origin Isolation Design

## Intent

Move privileged Platform pages, APIs, and the optional emergency terminal away
from the public Vapor website origin. A public-page script injection must not
be able to read or issue authenticated admin requests using the same browser
origin. This implements the isolation boundary already selected by the
Emergency Browser Console design.

## Goals

- Keep `vapor.ghf-studios.site` as the public website and service API host.
- Add an explicitly configured admin hostname, initially
  `admin.vapor.ghf-studios.site`, for Platform admin pages/APIs and Identity's
  browser login/session flow.
- Keep Identity's session cookie host-only; never widen it to the parent domain.
- Independently of hostname isolation, require a root Identity session before
  returning any `/admin` document, navigation, stylesheet, or script. Failed
  authorization returns a small standalone response without the admin shell.
- Redirect human login/admin entry points from the public host to the admin
  host when configured, while refusing admin API and terminal access from the
  public host.
- Leave current behavior in place until the admin hostname is configured, but
  fail closed for emergency-console enablement unless the admin origin is
  explicitly configured and differs from the public origin.
- Keep the Caddy route shared and service-owned business logic unchanged.

## Non-goals

- Provisioning DNS or changing the external DNS provider.
- Enabling or installing `ttyd`; the reviewed upstream release and console
  enablement blockers remain separate.
- Changing public account policy, Steam/GitHub linking, or Identity business
  logic beyond its canonical browser origin configuration.
- Treating sibling-subdomain isolation as protection against CSRF: all
  state-changing endpoints retain exact Origin validation and no credentialed
  permissive CORS.

## Approaches

1. **Dedicated admin hostname and host-only Identity session (selected).**
   Public and privileged pages have different browser origins. This matches
   the existing emergency-console security design and contains ordinary
   public-page XSS. It requires an owner-provisioned DNS record and coordinated
   Caddy/Identity origin configuration.
2. **Keep one origin and add a confirmation/reauth step.** This avoids DNS and
   is simpler to deploy, but same-origin script injection can still issue
   requests as the admin; a visible confirmation does not repair that boundary.
3. **Keep the current origin and disable the emergency console permanently.**
   This is safest in the short term but does not deliver the requested
   isolated privileged workbench.

The existing emergency-console design selected approach 1. `VAPOR_ADMIN_DOMAIN`
is optional during migration. Once it is configured, `/admin*`, admin APIs,
and terminal paths must no longer be served on the public host. Public `/login`,
`/logout`, and CLI SSH authorization entry points redirect to the admin host so
the browser session is created only there. On the admin host, the root path
lands at `/admin`; only the dashboard, Identity browser/API routes, and the
fixed terminal proxy are served.

## Constraints and ownership

- Platform owns public/admin host routing and the Platform origin used by its
  admin mutation guards.
- Identity owns login/session semantics and is configured with the admin host
  as its single canonical browser origin when isolation is enabled.
- Platform's shared shell gate verifies the root role before composing admin
  fragments or serving admin assets. Service APIs continue to authorize their
  own reads and mutations; a shell authorization is not a substitute.
- Caddy remains the only public gateway; independent service routing remains
  unchanged for public traffic.
- The admin host is not considered active until DNS, HTTPS, login callback,
  host-only cookie, authenticated admin health, and exact-Origin mutation tests
  all pass.
- Emergency console remains disabled until the isolated admin host is live and
  a fixed reviewed `ttyd` release is available.

## Acceptance

- With no admin host configured, current public routes continue to work and the
  emergency console cannot be enabled. `/admin` still requires root Identity
  authorization; a shared origin is not a reason to serve the operator shell
  publicly.
- Requests without a valid root session do not reach admin document/asset
  handlers and receive no admin navigation, layout, CSS, or JavaScript.
- When Identity denies the root role or is unavailable, the response has no
  admin shell and is not cacheable as a public page.
- With an admin host configured, public `/admin` redirects to that host,
  public admin APIs and terminal routes return 404, and public login entry
  points redirect while preserving the path and query.
- The admin host serves the dashboard, Identity login/callback/API flow, and
  admin APIs; the Identity cookie has no `Domain` attribute.
- Wrong-host and wrong-Origin state-changing requests are rejected.
- Caddy validates before replacing the live configuration; failed validation
  leaves the prior file untouched.
- Monolith and services Caddy templates behave identically at the host
  boundary.
