# Vapor Content Access Policy Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a reusable resource policy path that gates Loo-Cast premium content on its publisher-verified Steam license while representing Mad Max as a local external-source prerequisite.

**Architecture:** Identity remains authoritative for user sessions and verified entitlements. Registry stores each ecosystem resource's declared policy and gates every protected response through an authenticated Identity authorization call. The client may use local-source requirements to decide whether a resource is available on that machine; local files are never uploaded or treated as remote license proof.

**Tech Stack:** Rust 2021, Axum, SQLx/SQLite, Reqwest, Serde, existing platform-server service modules.

**Spec:** `docs/superpowers/specs/2026-11-01-vapor-content-access-policy-design.md`

## Global Constraints

- Vapor app downloads and explicitly public content stay public without Steam.
- Loo-Cast entitlement checks run only on the server with the publisher-authorized key.
- Mad Max is a locally acquired source dependency; do not make a remote Steam ownership check for it.
- Never distribute or upload Mad Max game files, extracted assets, or full local indexes.
- Identity owns verified permission facts; Registry owns resource policy and enforces it before returning content.
- Protected resources fail closed and must not be distinguishable from missing resources to unauthorized callers.
- Do not commit credentials, keys, or tokens; configure secrets through deployment environment.
- Keep service logic in its owning service crate; the platform root only wires service routes and configuration.
- This first slice gates the current Registry ecosystem lookup. It does not invent package, composition, or artifact routes that do not exist yet.

## Review Focus

- Missing, malformed, expired, revoked, or non-root user session: Identity denies and Registry hides protected resources.
- Steam license is temporary, shared, canceled, or provider response is incomplete: the Loo-Cast product's configured license mode determines the result; provider errors fail closed.
- Identity service is unavailable or internal service authentication is wrong: protected Registry requests fail closed while public records remain available.
- Protected resource is requested by its exact URL: response does not reveal whether it exists.
- Mad Max local mount is absent: client marks local-dependent content unavailable without sending game files or claiming server-side license proof.

---

### Task 1: Identity product-entitlement authority

**Files:**
- Modify: `Vapor-Identity-Server/src/config.rs`
- Modify: `Vapor-Identity-Server/src/providers.rs`
- Modify: `Vapor-Identity-Server/src/persistence.rs`
- Modify: `Vapor-Identity-Server/src/db.rs`
- Modify: `Vapor-Identity-Server/src/types.rs`
- Modify: `Vapor-Identity-Server/src/lib.rs`
- Create: `Vapor-Identity-Server/src/authorization_handlers.rs`
- Modify: `Vapor-Identity-Server/README.md`

**Interfaces:**
- Consumes: current Steam-anchored identity session cookie and the configured publisher key.
- Produces: `POST /v1/internal/authorization/check` accepting `{ "audience": "vapor-registry", "session_token": "…", "policy": { "mode": "all", "requirements": [{ "kind": "product_entitlement", "product_id": "loo-cast", "license_mode": "permanent" }] } }` and returning only `{ "allowed": bool, "expires_at_unix": integer }`.
- Product IDs map to provider product IDs in server configuration; the client cannot supply an App ID or provider key.
- The route requires a configured service-to-service bearer token and validates the session against Identity's current database before evaluating requirements.
- `loo-cast` uses the publisher-authorized Steam ownership lookup. `permanent` requires a permanent license; an `active` mode may be supported for products whose policy permits temporary/shared access.

- [x] Add configuration for the Loo-Cast Steam App ID and an internal authorization bearer token; never provide defaults for the secret.
- [x] Add a minimal cached ownership record keyed by Steam profile/product, including provider, license mode, checked time, expiry, and result; expire it after 300 seconds.
- [x] Implement a server-side Steam ownership provider using the publisher-authorized ownership endpoint; reject canceled, expired, and non-permanent licenses when the policy asks for `permanent`.
- [x] Add the internal authorization handler. Require the service bearer token, a valid non-revoked session, an allowlisted audience, and recognized requirement kinds. Do not return SteamID, profile ID, or provider response.
- [x] Add the route to the Identity API router and document the server-only environment values without sample secrets.
- [ ] Run `cargo check --manifest-path Vapor-Identity-Server/Cargo.toml` and `cargo check --manifest-path Cargo.toml`.
- [ ] Manually verify with the operator's configured credentials that owned, unowned, and unavailable-provider cases return the expected allow/deny result. Do not log or capture credentials.

### Task 2: Registry policy storage and enforcement

**Files:**
- Modify: `Vapor-Registry-Server/Cargo.toml`
- Modify: `Vapor-Registry-Server/src/db.rs`
- Modify: `Vapor-Registry-Server/src/types.rs`
- Modify: `Vapor-Registry-Server/src/lib.rs`
- Modify: `Vapor-Registry-Server/README.md`

**Interfaces:**
- Consumes: Identity's internal authorization endpoint and the browser/client's existing `vapor_identity_session` cookie.
- Produces: a versioned `ResourceAccessPolicy` attached to ecosystem resources with `visibility`, `permission_requirements`, and `local_requirements`.
- Public resources bypass Identity and retain current behavior. Protected resources call Identity before reading response details; denied and unknown resources both return the same generic `404` response.
- Local requirements are returned as metadata for client-side readiness checks; they are not sent as permission claims to Identity.

- [x] Add an idempotent SQLite migration for an explicit resource policy. Mark the seeded public `ghf-studios/vapor` ecosystem explicitly public; fail closed for invalid policy data.
- [x] Add typed policy and requirement structures with only the schema-defined variants; reject unknown schema versions and unknown requirement kinds.
- [x] Add Registry configuration for Identity's internal authorization URL and bearer token; protected resources fail closed if either is missing.
- [x] Update `get_ecosystem` to evaluate policy before returning resource metadata, forward only the session token over the server-only channel, and return generic not-found for denial.
- [x] Add `Cache-Control: private, no-store` to authenticated decisions and protected responses. Ensure public responses cannot cache user-specific authorization results.
- [ ] Keep provider-account lookups public until those resources receive an explicit policy; do not silently inherit ecosystem access rules.
- [ ] Run `cargo check --manifest-path Vapor-Registry-Server/Cargo.toml` and `cargo check --manifest-path Cargo.toml`.
- [ ] Manually verify a public lookup works unauthenticated, a protected lookup works for the entitled profile, and an unentitled/unauthenticated direct lookup returns the same not-found response.

### Task 3: Root routing and deployment wiring

**Files:**
- Modify: `src/main.rs`
- Modify: `deploy/systemd/vapor-platform-server.service`
- Modify: `deploy/README.md`
- Modify: `docs/cross-service-authorization-contract.md`
- Modify: `docs/vapor-standalone-install-and-source-home-contract.md`

**Interfaces:**
- Consumes: Identity and Registry environment configuration from Tasks 1–2.
- Produces: one documented internal authorization route, unavailable through unauthenticated client calls, with shared server-only credential configuration.

- [x] Route Identity and Registry through the shared Platform gateway without moving authorization business logic into Platform; the services remain independently deployable.
- [x] Add environment variable names and purpose to deployment docs/systemd environment configuration; use placeholders only and do not add real secrets.
- [x] Update the cross-service contract to distinguish product entitlement checks from local source prerequisites and to record the internal authorization request/response shape.
- [x] Update the standalone-install contract: public Vapor install/update remains ungated; Loo-Cast premium resources require the configured entitlement; Mad Max dependencies require a local mount only.
- [ ] Run `cargo check --manifest-path Cargo.toml`, `git diff --check`, and shell syntax checks for any changed deployment scripts.
- [ ] Review the deployment diff to confirm no key, token, private account data, or game file path was added.

### Task 4: Client integration boundary and content-service adoption contract

**Files:**
- Modify: `docs/superpowers/specs/2026-11-01-vapor-content-access-policy-design.md`
- Modify: `docs/decisions-and-backlog.md`

**Interfaces:**
- Consumes: the Registry policy response and Identity authorization behavior from Tasks 1–3.
- Produces: a compact contract for Vapor Client and future package/artifact/publishing services to reuse; no changes to the read-only Vapor Client checkout in this plan.

- [x] Record that the client may hide or disable Mad Max-dependent resources when the local source is absent, while the server does not treat local claims as proof of license.
- [x] Record that every future artifact/composition route must enforce the same permission policy before creating a stream or reusable URL.
- [x] Record that a future upload service assigns the authenticated Identity profile as canonical creator; client-supplied author metadata never proves ownership or grants update/delete permission.
- [ ] Keep upload storage, moderation, resource ownership, and owner-only update/delete endpoints out of this slice because no content-publishing service owns them yet.
- [ ] Track Vapor Client's local-mount presentation and content installer integration as a separate follow-up because that repository is not writable in this workspace.
- [ ] Confirm the current Registry's lack of package/artifact APIs remains an explicit scope boundary rather than a claimed implementation.

## Handoff limits

- No protected Loo-Cast resource should be published until Tasks 1–3 are deployed and verified.
- The current Registry has no package/composition catalog or download service, so this plan proves access enforcement on existing Registry ecosystem metadata; future content services must adopt the contract before exposing protected artifacts.
- Client-side local-mount checks require a separate writable Vapor Client workspace.
- The current Steam browser session is short-lived (300 seconds). This plan does not silently lengthen it; a production launcher experience will need a separately approved user-session lifetime/refresh policy.
- A local source prerequisite can hide or disable a resource in the user's client, but cannot securely control a direct remote API query without trusted machine attestation. No attestation is in this tranche.
- Deployment and production secret provisioning remain operator actions; this plan does not deploy the server.
