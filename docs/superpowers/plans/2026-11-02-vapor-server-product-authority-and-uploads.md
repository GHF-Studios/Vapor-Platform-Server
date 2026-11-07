# Vapor Server Product Authority and Content Upload Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend the existing server-owned access policy so first-party depot products cannot be impersonated as publishable user content, then add a separate service for authenticated user-content uploads.

**Architecture:** Registry owns canonical product/source policy. Identity already owns sessions, roles, and audience-bound authorization; this work adds only the content-upload audience. A new Content Upload service checks Registry policy and Identity authorization before storing bytes. Platform Server composes the service without taking its business logic.

**Tech Stack:** Rust, Axum 0.8, SQLx/SQLite, the existing Registry-to-Identity authorization contract, bounded streaming uploads, service-owned immutable storage, and Platform Server composition.

**Spec:** `docs/superpowers/specs/2026-11-02-vapor-standalone-product-boundaries-design.md` (Vapor Client checkout), `docs/superpowers/specs/2026-11-01-vapor-content-access-policy-design.md`, and `docs/cross-service-authorization-contract.md`.

## Global Constraints

- A local App.vapor.toml, content manifest, or package-provided publishable field is descriptive input only.
- Identity remains the authority for verified identity, roles, and server-side entitlement decisions.
- Registry owns canonical product IDs and versioned product/source policy records.
- The existing Artifact service remains Vapor application-release-only; user content uploads have a separate service.
- Steam publisher credentials and signing keys never ship in Vapor clients or manifests.
- The Platform Server composes and deploys service repositories; it does not own service business logic or databases.
- Reserved IDs prevent publishing canonical first-party products under their real identities, but do not guarantee detection of copied game bytes uploaded under a new identity.

## Review Focus

- Client manifest claims a depot-owned product is publishable — Registry returns server policy and upload is rejected.
- Unknown or conflicting product ID — upload fails closed without creating a record or storing bytes.
- Client forges an author field — Content Upload derives the author from the verified Identity decision.
- Identity or Registry is unavailable — upload fails closed and removes incomplete staging data.
- User payload contains copied proprietary game bytes under a new ID — do not claim perfect detection; fingerprint scan/review remains separate policy work.

---

### Task 1: Add server-owned first-party product/source policy to Registry

**Files:**
- Modify: `Vapor-Registry-Server/src/db.rs`
- Modify: `Vapor-Registry-Server/src/types.rs`
- Modify: `Vapor-Registry-Server/src/lib.rs`
- Test: Registry product-policy database and handler tests

**Interfaces:**
- Registry owns stable product records with `product_id`, `source_kind`, `publishability`, and `policy_version`.
- Seed the exact first-party IDs from the current Steam app manifest: `ghf-studios/loo-cast/spacetime-engine`, `ghf-studios/loo-cast/loo-cast-game`, and `ghf-studios/loo-cast/loo-cast-packagepack`; each is `steam_depot` and non-publishable.
- The Content Upload service can query policy through a loopback-only internal endpoint; browser/client-supplied values cannot alter policy.

- [ ] **Step 1: Add failing Registry tests** `local_only_products_are_seeded_non_publishable`, `unknown_product_policy_is_absent`, `duplicate_product_policy_is_rejected`, and `client_policy_fields_do_not_override_server_policy`; assert the three exact IDs are `steam_depot`/non-publishable and a user-product record remains publishable.
- [ ] **Step 2: Run focused Registry tests** and confirm the product-policy cases fail before implementation.
- [ ] **Step 3: Add an idempotent product-policy migration and typed policy records**; seed the three canonical Loo Cast product IDs as `steam_depot`/non-publishable.
- [ ] **Step 4: Add a server-authenticated internal policy lookup**; tests `product_policy_lookup_requires_service_token` and `policy_lookup_returns_minimal_fields` assert unauthorized callers fail and no Identity/session/private fields are returned.
- [ ] **Step 5: Run Registry formatting, tests, Clippy, and Rustdoc**; verify the existing ecosystem entitlement/local-source behavior remains unchanged.

### Task 2: Extend Identity authorization for Content Upload

**Files:**
- Modify: `Vapor-Identity-Server/src/authorization_handlers.rs`
- Modify: `Vapor-Identity-Server/src/lib.rs`
- Modify: `Vapor-Identity-Server/src/types.rs` or `profiles.rs` for the internal creator response
- Modify: `docs/cross-service-authorization-contract.md`
- Test: Identity audience/role authorization tests

**Interfaces:**
- Reuse the existing internal authorization request and service-token authentication.
- Add the allowlisted audience `vapor-content-upload`; require an authenticated session with `content-developer` for user-content submission.
- Add `POST /v1/internal/content-author` for the upload service. It accepts the validated session token under the service bearer token and returns `{ "creator": { "provider": "github", "provider_user_id": <stable GitHub user ID>, "login": "<linked login>" }, "expires_at_unix": <timestamp> }` only when the session is valid, GitHub is linked, and the profile has `content-developer`.
- Use linked GitHub numeric user ID as the canonical creator key; login is a display snapshot. Never expose the internal Identity profile row ID, raw token, or provider credential.

- [ ] **Step 1: Add failing authorization tests** `content_upload_requires_developer_role`, `content_author_requires_linked_github`, `unknown_audience_is_rejected`, `invalid_service_token_is_denied`, `identity_failure_denies`, and `creator_response_omits_profile_row_id`; assert only the linked developer gets the stable GitHub ID/login pair.
- [ ] **Step 2: Run focused Identity tests** and confirm the new audience is rejected.
- [ ] **Step 3: Add only the `vapor-content-upload` audience and its role requirement** to the existing authorization handler; keep `vapor-registry` behavior unchanged.
- [ ] **Step 4: Implement `/v1/internal/content-author`** with service-token validation, current-session lookup, linked-GitHub validation, and a no-store response containing only GitHub's stable provider user ID and display login.
- [ ] **Step 5: Update the cross-service contract** with the request policy, failure behavior, stable creator-key rule, and audience name.
- [ ] **Step 6: Run Identity tests, Clippy, and Rustdoc**; confirm all failures deny and the Registry audience remains compatible.

### Task 3: Create the separate Content Upload service

**Files:**
- Create repository/submodule: `Vapor-Content-Upload-Server`
- Create: `Vapor-Content-Upload-Server/Cargo.toml`
- Create: `Vapor-Content-Upload-Server/Workspace.vapor.toml`
- Create: `Vapor-Content-Upload-Server/src/lib.rs`
- Create: service-owned policy client, upload manifest types, storage code, and tests under `Vapor-Content-Upload-Server/src/` and `tests/`

**Interfaces:**
- A submission supplies a candidate product ID, manifest, declared length/digest, and streamed bytes; any publisher field is rejected/ignored.
- Before accepting bytes for publication, the service asks Registry for canonical product/source policy and Identity for the `vapor-content-upload` decision plus the canonical `github:<provider_user_id>` creator identity.
- Local-only or unknown product IDs are rejected. User uploads enter a review/publish state and are written only to Content Upload storage, never to Artifact release storage.
- Incomplete streams are bounded, staged, and removed on failure; accepted immutable content records include a server-computed digest and audit correlation ID.

- [ ] **Step 1: Add failing service tests** `upload_accepts_authorized_user_product`, `upload_rejects_depot_product`, `upload_rejects_unknown_product`, `upload_uses_identity_creator_not_request_author`, `upload_denies_identity_failure`, `upload_denies_registry_failure`, `upload_rejects_oversize_or_bad_digest`, and `upload_cleans_partial_stream`; assert denied requests persist no public record or bytes.
- [ ] **Step 2: Run focused service tests** and confirm policy, authorization, and storage paths fail.
- [ ] **Step 3: Implement bounded streaming and staged storage**; obtain Registry and Identity decisions, derive the publisher from the verified session, verify size/digest, then atomically promote an authorized complete upload.
- [ ] **Step 4: Add tests proving failures create no published record, partial files are removed, and Artifact release storage is never touched.**
- [ ] **Step 5: Run the service's full validation** and verify all policy, integrity, and cleanup cases pass.

### Task 4: Compose, deploy, and recover Content Upload

**Files:**
- Modify: `.gitmodules`
- Modify: `Cargo.toml`
- Modify: `src/main.rs`
- Modify: `Vapor-Platform-Server.vapor.toml`
- Modify: `deploy/scripts/build.sh`
- Modify: `deploy/scripts/deploy.sh`
- Modify: `deploy/scripts/health-check.sh`
- Modify: `deploy/scripts/install-systemd.sh`
- Modify: `deploy/README.md`
- Modify: `docs/operations-evidence-and-recovery-contract.md`
- Test: Platform router composition, health, and export/import tests

**Interfaces:**
- Public API is nested at `/api/content-upload/`; policy/admin internals remain service-owned and protected.
- The service exposes its own readiness and export/import contract. Platform deploys it independently and composes its export/import into whole-system recovery.

- [ ] **Step 1: Add failing Platform tests** `content_upload_routes_are_composed`, `content_upload_health_degrades_without_root_failure`, and `content_upload_state_is_in_export_import`; assert the router is mounted only under `/api/content-upload/` and recovery manifest includes only service-owned state.
- [ ] **Step 2: Run the focused Platform tests** and confirm Content Upload is not composed.
- [ ] **Step 3: Add `Vapor-Content-Upload-Server` as a root-level submodule** and wire router, service-owned state path, readiness, and independent build/deploy unit.
- [ ] **Step 4: Add recovery composition** that includes service-owned upload state but excludes `/etc` secrets and validates paths/sizes before restore.
- [ ] **Step 5: Run Platform tests and deployment-script validation**; verify independent rebuild/deploy and whole-system export/import.

## Reuse and limits

The existing `2026-11-01-vapor-content-access-policy.md` plan remains the
foundation for Loo-Cast entitlement and Mad Max local-source requirements. This
plan extends Registry with reserved product-source policy and adds upload
ownership/storage; it must not reimplement the already-established Identity
session or Registry entitlement flow. Reserved product IDs block direct
impersonation, but detecting copyrighted bytes renamed under a new product ID
requires a separate fingerprint/review policy.
