# Identity Project workspaces implementation

Completed Identity Project membership authority and the approved public/operator
session-purpose prerequisite. Local commits on the existing Identity branch:

- `43a2a59ada8ef77434dd4739d5dce2d4e7c238a9`: initial membership implementation.
- `1ae570e31d53f5d1ca8e3a5ad86da4695176b107`: reviewed credential-status, disabled
  profile revoke, and stable Project-ID fixes.
- `2b0db42a1f79a563365bcfab1c7d9e0b50dd5592`: explicit public/operator session
  purposes, login/callback routes, and isolation tests.

No push, secrets, provider policy changes, or root orchestration changes.

## Membership API contract

- Migration 9 adds `project_memberships`, keyed by `(project_id, profile_id)`;
  roles exactly `owner`, `developer`, `viewer`, with viewer < developer < owner.
  Grants/revocations persist actor, timestamps, and optimistic version. Global
  root/developer roles imply no Project membership; no memberships are seeded.
- Project IDs match Registry's lowercase `namespace/slug`, each component 1–63
  ASCII letters/digits, `_`, `-`. Actual first-party IDs are tested. Schema and
  HTML form validation accept the separator and reject extra separators/XSS.
  The unreleased schema 9 constraint was corrected directly; no live v9 data was
  migrated or discarded.
- `POST /v1/internal/projects/list`: `{audience, session_token}` returns
  `{memberships: [{project_id, role}], expires_at_unix}`.
- `POST /v1/internal/projects/check`: adds `{project_id, minimum_role}` and returns
  `{allowed, role?, expires_at_unix}`. Only audience `vapor-registry` and its
  dedicated bearer service credential are accepted. Session rows must have
  purpose `user`, be live/unrevoked, and attach to an enabled profile. No
  profile/provider data or session tokens appear in responses. Expiry is
  min(session expiry, now + 30s).
- Malformed body/audience is 400; invalid service credential is 403; inactive or
  wrong-purpose user session is 401; unavailable storage is 503. Registry must
  map upstream 403 to outage (503), and caller-session 401 to signed-out.
  Project responses, including request rejection, use `private, no-store`.
- Root-session JSON mutations at `/v1/admin/project-memberships/grant` and
  `/revoke` accept `{project_id, profile_id, role, expected_version, confirmed}`.
  Same-origin and explicit confirmation are mandatory. New rows use version 0;
  successful changes return `{version, changed}`. Stale versions return 409;
  matching idempotent operations add neither a version nor fake audit event.
  Changes and actor/subject audit events commit atomically. Grant requires an
  enabled target profile; revoke also works for disabled target profiles.
- `GET /admin/identity/project-memberships` provides the root-only management
  fragment with `application/vnd.vapor.admin-page.v1+html`, plus grant/revoke form
  POST routes beneath that path. The existing Identity fragment links to it.
  Displayed values are escaped. Parent confirmed Platform already forwards the
  Identity admin path prefix through its fixed fragment gateway.
- `/v1/export` remains the existing metadata/count manifest, now including
  `project_memberships`; existing audit count includes membership audits. Active
  and revoked memberships plus audit records survive composed SQLite snapshot/
  restore. No row-import or secret export API was introduced.

## Public/operator session split

- Migration 10 adds constrained `user|operator` purpose to identity sessions.
  Existing and API-issued root sessions remain `operator`. Migration preserves
  existing sessions and browser attempts and is idempotent.
- Public `/login`, `/login/steam`, `/login/steam/callback`, `/login/github`,
  `/login/github/callback`, and `/logout` use host-only `vapor_user_session`.
  Verified Steam login grants no global roles and redirects to `/dashboard`.
- Operator equivalents live under `/operator/login` and `/operator/logout`, use
  host-only `vapor_identity_session`, and retain existing linked Steam + GitHub +
  root authorization. Both session types retain the existing 300-second TTL.
- Root/admin lookup rejects user-purpose tokens even in the operator cookie;
  Project membership lookup rejects operator-purpose tokens. Public login pages
  have no operator navigation, including for a root profile.
- Browser flow records, state cookie names, and callback-scoped cookie paths
  separate public from operator attempts. Return-to and flow checks prevent a
  callback from switching purpose. Optional GitHub linking still requires the
  corresponding existing Steam session; verification/provider policy is intact.
- `VAPOR_IDENTITY_PUBLIC_ORIGIN` now specifies the public account origin;
  `VAPOR_IDENTITY_OPERATOR_ORIGIN` specifies operator callbacks and admin Origin
  guards. Operator falls back to public origin for one-host setups. No Domain
  cookie attributes. README documents paths, config and callback registration.

## Verification

Membership fix commit:

- `cargo test --locked`: 51 passed, 0 failed.
- `cargo check --locked`: passed.

Final session-purpose commit:

- `cargo test --locked`: 57 passed, 0 failed (full Identity suite).
- `cargo check --locked`: passed.
- `git diff --check`: passed before commit.

Tests cover migration/idempotence, grants/revokes and version conflicts, audit
rollback, disabled-profile revocation, actual Project IDs, role hierarchy and
cross-Project denial, session revocation/expiry/disablement, service credentials,
root/origin/confirmation guards, malformed HTTP requests/private cache, fragment
media type, SQLite snapshot restore, legacy session migration, purpose isolation,
public/operator callback origins/state paths, cookie separation, public root
account rendering, and logout isolation. The snapshot cleanup test permits the
asynchronous SQLite worker to release its Windows file handle after pool close.
Existing unrelated warning remains: unused `DEFAULT_BIND` in `src/config.rs`.

## Integration checks remaining

Parent owns public/operator gateway routing and deployment origin configuration.
Provider callback registration must permit the deployed public/operator URLs;
existing GitHub client credentials/verification policy were not changed. No live
Steam/GitHub callback was executed against production providers. Registry must
forward only the public `vapor_user_session` as membership `session_token` and
recheck membership for every private Project request.

Submodule working tree was clean after committing. Root status was inspected;
other agents' root/submodule work was left untouched. Parent integration review
remains applicable to the session split.
