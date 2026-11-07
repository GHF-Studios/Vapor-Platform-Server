# Vapor Public Homepage

## Intent

Make `vapor.ghf-studios.site/` a clear first stop for someone who has not used
Vapor. Explain what the platform is, where setup begins, and that game files
come from the user's own legitimate local installation. `/downloads` remains
the canonical install and getting-started flow.

## Goals

- Give the homepage a polished, responsive presentation that matches the
  existing dark teal visual language on `/downloads`.
- Put one primary action, `Get Vapor`, on the page and route it to `/downloads`.
- Explain Vapor's purpose and the local-game-content boundary in plain language.
- Keep links to documentation and existing product/legal pages easy to find.
- Use semantic HTML, visible keyboard focus, and no external asset or script
  dependency.

## Non-goals

- Do not duplicate installer selection, release status, or setup instructions
  already owned by `/downloads`.
- Do not claim a stable release, store integration, or compatibility state
  unless the existing release/setup surfaces report it.
- Do not invent license or legal-contact details.

## Approaches

1. Keep the link-only page. This is the least work but does not serve as a
   useful public introduction.
2. Put the full install flow on `/`. This exposes more information, but creates
   two copies of release and setup behavior that can drift.
3. Add a concise product overview with a single route into `/downloads`.
   Selected because it improves the first visit while keeping release and
   installation ownership in one canonical page.

## Constraints

- Homepage service owns public product and legal copy; Platform owns routing.
- Keep the service stateless and independently rebuildable.
- The `/downloads` route remains authoritative for current installer status.
- Do not include proprietary game files or invite users to upload them.
