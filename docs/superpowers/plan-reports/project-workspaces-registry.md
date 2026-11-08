# Registry Project directory implementation

Completed Task 3 in `Vapor-Registry-Server` only. Local commits:
`8b606eec7c30106696a634ecf1e2bcbb4c095ce7`
(`Add membership-gated Registry Project directory APIs`) and
`2c7030f72792aa8940dfb878d21838ece1080b3e`
(`Expand Project credential and metadata boundary coverage`), then final HEAD
`ea6ae627c0ea694d95f5e718d4260be693b062c2`
(`Preserve member access to archived Projects`). No push.

## Changes and contract

- Registry schema version 3 adds distinct `projects` metadata with exact
  `namespace/slug` IDs, namespace, slug, display name, summary, and
  `active|archived` lifecycle. Database checks enforce identity, allowed
  characters, metadata bounds, uniqueness, and lifecycle values.
- Idempotent bootstrap seeds active `ghf-studios/loo-cast` (Loo Cast) and
  `ghf-studios/mad-max-modding` (Mad Max Modding). Existing Project edits or
  archival are retained on restart. No membership is seeded, source repository
  is attached, or ecosystem behavior is changed.
- `GET /v1/me/projects` returns `{projects:[{id, namespace, slug, display_name,
  summary, state, role}]}` by composing active or archived Registry metadata with
  Identity's current membership list. Unknown Registry records are omitted.
  Archived Projects remain member-readable and read-only; no matches is a
  successful empty list.
- `GET /v1/projects/{namespace}/{slug}` rechecks Identity membership at minimum
  `viewer` on every request, then returns `{project:{id,namespace,slug,
  display_name,summary,state},membership:{role}}` for active or archived Registry records.
- Both APIs accept only `vapor_user_session`, rejecting missing, malformed,
  oversized, and duplicate session cookies. Operator cookies and browser bearer
  headers grant no access. Global/root roles are never interpreted as membership.
- Project routes use `private, no-store` middleware, including extraction,
  method, authentication, and failure responses. Missing/invalid user sessions
  map to 401; absent membership and unknown Projects use generic 404;
  Identity configuration, bad service credential, outage, expired decision,
  invalid schema/ID/role, or other protocol errors map to 503.
- Identity client uses the configured dedicated Registry bearer token and exact
  `vapor-registry` audience, forwarding only the user session token. It derives
  `/v1/internal/projects/list` and `/check` from the existing authorization URL
  suffix and preserves a monolith `/api/identity` prefix. Loopback HTTP only,
  no redirects or proxies, existing 1s connect/3s request timeouts. Response
  reads bound Content-Length and streamed chunks to 64 KiB; lists bound to
  512 memberships, with duplicate IDs rejected. Decisions are not cached.
- README documents Project APIs separately from source-topology APIs and notes
  JSON metadata must be rendered as text by browser consumers.

## Verification

- `cargo test --locked`: 10 passed, 0 failed, including 5 new Project tests.
- `cargo check --locked`: passed, no warnings.
- `git diff --check`: passed (only Git line-ending normalization notices).
- Migration test was observed failing on `no such table: projects` before
  implementation, and then passing. API test was observed failing with 404
  instead of required 401 before route implementation, and then passing.
- Coverage: idempotent seed, metadata/lifecycle constraints, ecosystem isolation,
  signed-out/root-cookie rejection, exact bearer/audience/session/check request
  body, membership/metadata intersection, role and HTML label preservation,
  empty success, revoked/cross-Project denial, unknown Project hiding even when
  Identity allows, archived Project list/detail access for current members, per-request decision rechecks, upstream 401/403/error mapping,
  expired/malformed/oversized responses, unsafe URLs and tokens, standalone and
  monolith endpoint derivation, chunked-response size bound, and outage handling.
- Self-reviewed the entire Registry diff against the plan and request. No
  subagent was spawned, per delegated scope. Parent integration review remains
  applicable. Registry working tree is clean after the local commit.

## Rulings and integration concerns

- Project components use lowercase ASCII `[a-z0-9_-]`, 1–63 bytes each, giving
  exact `namespace/slug` IDs up to 127 bytes. Identity must use the same contract;
  the parent was notified to align its original opaque/no-slash ID validator.
- The parent specified the upstream distinction: Identity 401 means invalid
  user session; Identity 403 means invalid Registry service credential. Older
  Identity builds that return 401 for both cannot preserve this failure split;
  deploy the aligned Identity change with this Registry commit.
- Registry SQLite backup/restore already preserves the new table as part of
  the whole database; this task adds no separate export/import mechanism.
- This task implements no creation/edit API, invitations, or content workbench.
  Initial memberships must be granted explicitly in Identity administration.
- Integration correction: the approved design explicitly preserves archived
  Project access for current members. The Task 3 plan sentence saying only
  active Registry records should be listed was wrong. Updated that plan sentence,
  README, and tests; removed active-only database filtering. The amended API
  test first failed with 1 returned Project versus 2 expected, then passed after
  the fix, including archived-state list/detail assertions. Full Registry tests
  (10 passed), check, and diff checks passed again before the corrective commit.
  No Project write API exists, preserving read-only archived access.
- No Identity, Homepage, or Platform implementation files were changed. The
  root report and the corrected Task 3 plan sentence are the sole root files
  written by this task.
