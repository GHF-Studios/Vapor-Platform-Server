# Vapor Content Access Policy Design

Status: approved direction with owner clarification; no policy enforcement is
implemented by this document.

## Intent

Vapor needs one reusable model for deciding whether a person may discover,
query, resolve, download, or activate a content resource. The policy must work
for the Vapor ecosystem generally, including premium-product entitlements,
publisher grants, Vapor capabilities, and content dependent on locally acquired
external sources. Loo-Cast is a premium product whose access can be verified
with its publisher-authorized Steam entitlement. Mad Max is an external local
source dependency; Vapor requires the player's local game files when needed,
but does not ask Steam to verify Mad Max ownership as a remote entitlement.

Vapor's own installer, updates, account access, and explicitly public content
remain public unless their own policy says otherwise. Vapor must never distribute
Mad Max or another product's executable, archives, extracted assets, or other
proprietary source data. An access gate controls Vapor content; it does not by
itself establish permission to redistribute third-party material.

## Goals

- Let content declare structured requirements using stable product, provider,
  role, grant, and local-mount identities rather than UI-only flags.
- Make Identity authoritative for verified identity and permission facts.
- Provide the identity basis future publishing services can use to bind an
  upload to its authenticated author; the server, never client metadata, sets
  the canonical author identity.
- Require each content-owning service to enforce policy at every access path,
  including listing/search, detail, dependency resolution, version/manifest,
  and artifact download routes.
- Allow public content and independently gated content to coexist in the same
  ecosystem without hiding Vapor itself.
- Keep machine-local availability checks local: they can gate local install,
  launch, or activation, but are not proof of a license to a remote service.
- Fail closed for a protected operation when authentication or authorization
  cannot be established, and avoid revealing protected resource existence.
- Preserve provider flexibility. Steam can verify a publisher-owned premium
  product where Vapor has the required authorization; it is not the format's
  only provider.

## Non-goals

- Distributing, mirroring, or extracting proprietary product files on the VPS.
- Treating a local file marker, client-supplied ownership boolean, or unsigned
  local report as authoritative proof of a remote license.
- Making Steam or Loo-Cast access a requirement for installing or updating
  Vapor itself.
- Assuming that owning a game automatically grants rights to redistribute
  third-party game data or any particular mod package.
- Building a generic policy language with arbitrary scripts or user-defined
  executable predicates.
- Implementing an upload, ownership-transfer, edit, or delete API before a
  content-publishing service exists.

## Current facts and mismatch

- `Vapor-Identity-Server` owns identity, sessions, roles, and authorization
  decisions. Its current Steam ticket verifier authenticates against the
  configured Vapor Steam app ID (`2122620` by default); this does not prove
  ownership of Mad Max or another product. The current browser Steam login
  creates a short-lived identity session, while the separate auth-attempt flow
  is root-dashboard-specific; a general content entitlement session contract
  still needs implementation.
- The Registry currently exposes public ecosystem and provider-account
  metadata. It has no package catalog, entitlement filter, or binary download
  endpoint yet.
- `docs/cross-service-authorization-contract.md` now records the first
  audience-bound Registry-to-Identity decision transport. Other services and
  artifact routes do not yet consume it.
- Owner clarification: Loo-Cast is a publisher-owned premium product and may
  require a verified Loo-Cast entitlement. Mad Max remains an externally
  acquired local source; its Steam distribution is incidental to the dependency
  model. Vapor never hosts or redistributes its game files, and local
  availability is checked by the client.
- Valve documents `ISteamUser/CheckAppOwnership` as requiring the publisher API
  key that owns the specified App ID, with the call made server-side. Therefore
  a Vapor Steam login alone is not proof of product entitlement. The owner
  confirms Vapor has the publisher-authorized key for Loo-Cast, so the first
  entitlement integration can target that product using its publisher-owned
  App ID. The key must never be exposed to the client. See [Valve's
  CheckAppOwnership documentation](https://partner.steamgames.com/doc/webapi/ISteamUser)
  and [AuthenticateUserTicket documentation](https://partner.steamgames.com/doc/webapi/isteamuserauth).

## Approaches considered

1. **Client-only gating.** The client hides or disables content based on local
   state. Rejected as the security boundary: direct requests bypass the UI, and
   client reports can be forged.
2. **Independent policy logic in every service.** Rejected as the primary
   model: services would duplicate provider verification and likely disagree
   about grants, expiry, revocation, and unknown requirements.
3. **Identity-owned permission facts with service-enforced resource policies —
   selected.** Identity verifies provider proofs and grants, then answers
   audience-bound authorization requests. The service that owns a resource
   stores its requirement policy and enforces it before returning that
   resource. This keeps Identity authoritative for permissions while keeping
   resource metadata and route enforcement with the correct service.

## Policy model

Each resource policy is explicit and schema-versioned. It distinguishes:

- **Permission requirements:** server-verifiable grants such as a product
  entitlement from an authorized provider, a Vapor role/capability, or a
  publisher-issued grant. Requirements support a small, deterministic `all` or
  `any` composition; arbitrary executable policy is out of scope.
- **Local prerequisites:** machine-local sources or mounts, optionally with
  version/build constraints. These determine whether the client can install,
  activate, or launch the content against local files.
- **Visibility:** whether metadata is public or protected. Protected metadata
  must be filtered server-side before it is returned. A missing policy on a
  newly published resource is an error; existing explicitly public records
  need an explicit migration to the public policy.

The permission and local-prerequisite sets remain distinct. A permission
decision can be checked remotely; local presence is checked by the client. If
both are required, the client must satisfy both before install/activation.
Client catalogs and local workbenches may hide or mark unavailable resources
whose local source is absent. A remote service cannot securely enforce a
machine-local prerequisite without trusting client-reported state; this design
does not add machine attestation. The client must not upload game files,
extracted assets, or full local indexes to prove a local prerequisite.

## Authority and request flow

```text
provider proof / authorized grant
              |
              v
       Vapor Identity
       permission facts
              |
     audience-bound decision
              v
   resource-owning service ---- resource policy
              |
    filter or deny every route
              v
       Vapor Client
       local prerequisite check
              |
       install / activate
```

Cross-service requests must be authenticated as a known service audience and
tied to the current user session. The current Registry integration uses a
server-only bearer secret and loopback HTTP; a browser/client must not submit
its own policy or permission facts. Identity returns only the minimum decision
needed by that audience, with short expiry and no raw tickets, provider
secrets, session IDs, or internal profile IDs. Steam entitlement decisions
are cached for 300 seconds; session validity is checked on every decision.

Protected resources should be indistinguishable from absent resources to an
unauthorized caller (for example, a generic not-found response). Responses
containing user-specific access results must not be shared through public
caches. A failure to reach Identity must fail closed for protected routes;
explicitly public resources remain available.

## Ownership boundaries

- **Identity:** verifies supported provider proofs, stores permission grants,
  evaluates authorization requests, and owns their expiry/revocation.
- **Registry/catalog owner:** owns content metadata and its declarative access
  policy; filters every discovery/query/dependency route using Identity's
  decision.
- **Artifact/download owner:** independently verifies authorization before
  revealing or streaming protected artifacts; a catalog decision is not a
  reusable public download URL.
- **Future publishing service:** binds the authenticated Identity profile to
  the new resource at creation time. Before uploads exist, Identity must expose
  a service-scoped way to resolve a current session to a stable opaque creator
  subject; the current allow/deny endpoint intentionally does not return that
  subject or internal profile IDs. Client-supplied author names are display
  metadata only; they cannot establish ownership or authorize edits/deletion.
- **Vapor Client:** presents available/locked state, checks local mounts, and
  prevents installation/activation when local prerequisites are absent. Its
  checks improve UX but do not replace server authorization.
- **Platform root:** routes and deploys service modules; it does not own content
  policy or entitlement business logic.

## Migration and compatibility

- Existing public Vapor app-release routes remain public and separate from
  product-dependent content.
- Existing public Registry records remain public only through explicit public
  policy; introducing protected records requires the enforcement path to be
  active before those records are published.
- The Registry ecosystem lookup now has an authenticated Identity decision
  path. Every future catalog/artifact route must adopt its own stored policy and
  enforce it before listing, resolving, or streaming protected resources.
- Loo-Cast is the first premium-product entitlement target and may use the
  publisher-authorized Steam ownership API. Mad Max is represented as an
  external local source requirement and does not need a Steam ownership query.
- Local mount validation remains a client feature and can be built without
  waiting for remote entitlement checks.

## Acceptance criteria

- A resource can declare public or protected visibility plus independent
  permission and local-prerequisite requirements.
- A Loo-Cast-protected resource is unavailable through every server query path
  unless Identity has confirmed the current user's configured Loo-Cast license
  mode (active access or permanent purchase).
- A Mad Max-dependent local composition cannot be installed or activated by
  the client unless its required local game source is mounted.
- An unauthorized user cannot enumerate, search, query, resolve dependencies,
  fetch a manifest/version, or download a protected resource by changing the
  URL or calling the API directly.
- Public Vapor downloads are unaffected by product-specific entitlement gates.
- Expired, missing, revoked, or unverifiable permission fails closed.
- Missing local files prevent local install/activation without being mistaken
  for a server-verified permission failure.
- No game files or extracted proprietary assets are uploaded or served by
  Vapor.
- Provider-verification limits and the configured source for each product
  entitlement are visible in operator configuration and never exposed as
  secrets.
- Future content uploads receive a server-assigned creator identity, and later
  owner-only operations check that identity rather than a client-supplied name.

## Open implementation decisions

- Which content type or catalog route should first consume Loo-Cast entitlement;
  current Registry has no package/composition catalog.
- Which future service should own content uploads, versions, moderation, and
  author-controlled update/delete operations; those APIs remain out of scope
  until that service boundary is approved.
- What service-scoped opaque creator subject Identity should return to a future
  upload service, without exposing internal profile IDs to clients or unrelated
  services.
- Whether loopback HTTP plus the shared environment-file bearer secret remains
  sufficient as service topology grows beyond one VPS/process boundary.
- Whether publisher grants are managed through an existing publishing
  workflow or a later admin surface.
- Which content service first owns package/artifact metadata and download
  routes; the current Registry is not yet that service.
