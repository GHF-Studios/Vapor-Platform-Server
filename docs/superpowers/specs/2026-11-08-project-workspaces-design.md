# Project Workspaces and Membership Design

**Status:** Implementation direction approved by the owner as part of the Project-centered Vapor model.
**Date:** 2026-11-08

## Intent

Give signed-in users a personal dashboard that lists only Projects they can
access, and give each Project a stable workspace route. Project identity must
not be inferred from a repository, Steam app, packagepack, or source-topology
ecosystem. Registry owns Project metadata; Identity owns membership and access.
The dashboard is a composition of those APIs, not another database.

## Approaches considered

1. Reuse Registry `ecosystems` as Projects. Rejected: those rows describe
   rebuild/source topology and do not have Project identity or membership.
2. Put Project metadata and membership in Identity. Rejected: Identity must not
   become the catalog/content owner.
3. Add Project records to Registry, per-Project grants to Identity, and a thin
   user-facing dashboard that composes their APIs. Selected: it follows the
   accepted ownership boundary and keeps each service independently deployable.

## User-visible slice

- `/dashboard` is a signed-in account landing page. It lists the current
  account's accessible Projects and links into each workspace.
- `/projects/{namespace}/{slug}` is a Project-scoped workspace landing page.
  This first slice shows Project identity and the user's Project role; typed
  content, releases, compositions, and Project-specific tools follow from
  Registry's catalog work.
- A public visitor can load the page shell, but receives no private Project
  records without a valid public-user Identity session and active membership.
- Project links are stable Vapor routes, independent of provider URLs.

## Session boundary required by the public workspace

- Public accounts use `/login` on the public Vapor host and receive a distinct,
  host-only `vapor_user_session` cookie. The current owner preference is
  Steam-required sign-in; normal account sessions do not require a global role.
- Operator authentication remains on the operator route (`/operator/login`)
  and receives the existing host-only `vapor_identity_session` cookie. It keeps
  its current root checks and stronger linked-provider requirements.
- The public and operator cookies are never scoped to a parent domain or reused
  across hosts. When the optional admin hostname is configured, the operator
  session stays on that host; the public-user session stays on the public host.
- Identity callbacks and return URLs are selected for the matching public or
  operator route. Registry accepts only the public-user cookie for Project
  membership; root does not silently become a Project member.

## Ownership and API contracts

- Registry owns a distinct `projects` table keyed by a stable Project ID with
  namespace, slug, display name, summary, and lifecycle state. Existing
  `ecosystems` remain unchanged. First-party seed data includes Loo Cast and
  Mad Max Modding as Projects; source topology is not implicitly attached.
- Archived Projects remain visible to current members as read-only workspaces
  with an `archived` state. Identity does not look up Registry lifecycle state;
  a root operator may grant a membership for any stable Project ID, while
  unknown IDs remain inert unless Registry owns a matching Project. The first
  slice has no Project mutation API.
- Identity owns active Project membership grants keyed by `(project_id,
  profile_id)` with role `owner`, `developer`, or `viewer`, grant/revoke state,
  actor, timestamps, and audit events. Global `root` and `content-developer`
  roles do not imply Project membership.
- Root Identity operators can grant/revoke membership through Identity-owned
  admin APIs. A grant references an opaque Registry Project ID; Identity does
  not query Registry. The initial UI can accept the stable ID directly.
- Membership and audit rows remain in the Identity SQLite database and are
  preserved by the composed database backup/restore. Identity's current export
  is a metadata/count report, not a row-level serialization format.
- Registry uses its existing server-only Identity audience credential and
  session cookie to ask Identity for the caller's active memberships. The
  internal response contains only Project IDs, roles, and authorization expiry;
  it does not expose session tokens or profile/provider details.
- Registry exposes account-scoped `GET /v1/me/projects` and a membership-gated
  `GET /v1/projects/{namespace}/{slug}`. Both are private/no-store; missing,
  revoked, or unknown membership returns a generic not-found response.
- The dashboard is a thin presentation layer and calls the Registry APIs. It
  does not copy Project/member records or call internal Identity endpoints.

## Non-goals

- Project creation, invitations, member self-service, content publication,
  compositions, provider acquisition, or project-specific UI plugins.
- Automatic Project ownership from global roles or from repository ownership.
- New identity providers, account migration, or changes to current login policy.
- A general-purpose cross-subdomain login cookie or sharing operator sessions
  with public pages.
- Moving Project or membership state into Platform, Homepage, or the browser.

## Failure and security behavior

- An unavailable Identity decision service fails closed and is distinguishable
  from an empty Project list.
- Every Project data API rechecks membership; hidden navigation is not access
  control.
- Public-user and operator sessions are purpose-separated and host-only; a
  public session cannot retrieve the root operator shell.
- Identity admin mutations require a root session, same-origin request,
  explicit confirmation, and audit the actor and change.
- Project role values and IDs are allowlisted and bounded; all browser-rendered
  metadata is escaped or inserted as text.

## Implementation choices left to the builder

- Use the existing SQLx/SQLite migration and test patterns in each service.
- Seed first-party Project metadata idempotently. Do not seed a member; the
  owner grants access explicitly through Identity administration.
- Implement the user dashboard/workspace shell in the existing public
  presentation service with static route shells and owner API data. Keep `/admin`
  and its operator assets separate.

## Acceptance evidence

- Registry migrations create Project metadata without changing ecosystem rows.
- Identity grants, revokes, project listing, and authorization survive restart,
  are included in export/import, and produce audit evidence.
- Registry returns only active memberships for valid sessions; revoked or
  cross-Project access is denied with private/no-store responses.
- The user dashboard shows only returned Projects, handles sign-in and service
  failures clearly, and each workspace displays the stable Project identity
  and role.
- No operator navigation, scripts, or data appear in user pages.
