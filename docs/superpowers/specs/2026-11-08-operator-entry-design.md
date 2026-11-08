# Operator Sign-in Entry Design

## Intent

Make the existing Platform admin area discoverable to its owner while keeping
it distinct from the public Vapor account flow. A normal Vapor sign-in creates
a player session; `/admin` requires the separate operator session and the
Platform root role.

## Approaches

- Reuse the player session for `/admin`: rejected because it weakens the
  deliberate session-purpose boundary.
- Leave the separate session but link only to `/login`: rejected because it
  loops users through the wrong sign-in flow.
- Keep player and operator sessions separate, link to `/operator/login` from a
  discreet homepage footer entry, and make admin auth failures explain the
  distinction: selected because it preserves authorization while fixing
  discoverability and misleading guidance.

## Scope and constraints

- The homepage owns only the discreet public entry link; it does not expose
  admin data or implement identity behavior.
- Identity owns session purpose and Platform authorization; both explain that
  normal account sign-in does not establish an operator session.
- Admin pages remain protected by Identity authorization and the root role.
- No cookie/session compatibility or silent elevation is introduced.

## Acceptance

- The homepage footer links to `/operator/login`.
- An unauthenticated `/admin` response links to `/operator/login`, not `/login`,
  and explains the separate operator session.
- Existing route protections remain unchanged.
