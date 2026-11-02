# Cross-service authorization contract

Status: Registry, Platform, and Artifact use separate audience-specific
Identity authorization tokens for root-gated operations. Docs and Diagnostics
still retain their migration-era service tokens.

## Purpose

Identity owns people, linked external identities, sessions, roles, and effective
role evaluation. Other services own their own resource policy.

This contract prevents Docs, Diagnostics, Publishing, Artifacts, Registry,
Toolchain, MCP, or operations tooling from copying the identity database,
inventing parallel roles, or treating a server-local admin token as normal user
authority.

## Authority split

```text
identity proof
  != Vapor role
  != service-local resource permission
  != publishing approval
  != pipeline execution authority
  != Steam publishing credential authority
  != deployment/recovery authority
```

`root` implies `content-developer`, but it does not automatically imply Steam
publishing authority, deploy authority, restore authority, or credential
custody.

## Current Registry flow

The first implementation is an audience-bound authorization decision:

```text
Registry receives request
  -> Registry extracts the Vapor session cookie and loads its resource policy
  -> Registry sends the session token and policy to loopback Identity
  -> Identity verifies the current session and evaluates permission requirements
  -> Registry returns the resource only when allowed
```

Each downstream service uses its own random server-only bearer token stored in
protected environment files. Identity selects the token by allowlisted
audience; Registry, Platform, and Artifact cannot present one another's tokens
or ask for another service's admin decisions. Downstream services accept only
a literal loopback Identity URL and never receive the Steam publisher key.
Identity returns a minimal decision, not profile facts.

## Authorization request

```json
{
  "audience": "vapor-registry",
  "session_token": "…",
  "policy": {
    "mode": "all",
    "requirements": [
      { "kind": "product_entitlement", "product_id": "loo-cast", "license_mode": "permanent" }
    ]
  }
}
```

Future audiences should be named after service authority boundaries, not routes.

The Artifact administration section uses audience `vapor-artifact-admin` and
the `root` role. It is read-only and reports staged files and metadata checks;
it does not verify signatures cryptographically, inspect private keys, or
promote channel pointers. Its token is distinct from Registry and Platform.

The Build Cache administration section uses audience `vapor-build-cache-admin`
and the `root` role. It is read-only and displays current target snapshots and
the last successful promotion identity. The builder pipeline—not the browser—
owns snapshot replacement. Its Identity service token is distinct from
Registry, Platform, Artifact, and the builder orchestrator token.

## Successful response

```json
{ "allowed": true, "expires_at_unix": 1784930300 }
```

The response must not include:

- internal profile IDs;
- session IDs;
- token hashes;
- raw tokens;
- provider tokens;
- Steam tickets;
- database row identifiers.

## Failure behavior

- Missing, invalid, expired, revoked, or disabled sessions produce a generic
  `{ "allowed": false }` decision.
- A missing/invalid service token returns `401`; an unsupported audience or
  malformed policy returns `400`.
- An unavailable Steam ownership provider returns `503`, so Registry denies
  the protected request rather than treating uncertainty as permission.
- Decision responses use `Cache-Control: no-store`; protected Registry
  responses use `Cache-Control: private, no-store`.
- The server-local Identity admin token is not an identity session and is not
  accepted by this endpoint.

## Downstream service policy

Identity returns facts. It does not decide every resource-specific action.

Initial downstream policies:

| Service action | Required downstream policy |
| --- | --- |
| Docs upload/export | effective `root` plus linked Steam and GitHub, or existing emergency docs token during migration |
| Diagnostics list/download/export | effective `root` plus linked Steam and GitHub, or existing emergency diagnostics token during migration |
| Diagnostics upload | explicit opt-in; identity is optional unless product policy changes |
| Publishing request | Steam profile + linked GitHub + required Vapor role + separate publishing/pipeline authority |
| Platform overview/operations status | `root` role through the `vapor-platform-admin` audience; read-only health and latest deploy/restore status |
| Artifact release inspection | `root` role through the `vapor-artifact-admin` audience; read-only metadata |
| Build Cache target inspection | `root` role through the `vapor-build-cache-admin` audience; read-only current snapshot and promotion metadata |

## Migration rule

Docs and Diagnostics may keep their local admin-token scaffolds during the
migration, but those tokens are bootstrap/emergency compatibility paths. They
are not the long-term operator workflow.

Registry enforces protected ecosystem reads and its root-only Workshop admin
view through Identity. Artifact's root-only inventory also checks Identity.
Docs and Diagnostics still need their own resource policies and authorization
integration before replacing their migration-era service tokens.
