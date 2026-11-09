# Deployment request lifecycle implementation

**Goal:** Make deployment approval and execution ordering agree across the web
API and root worker, and make each visible queue count mean exactly what it
says.

## Completed work in this pass

1. Keep currentness derived from verified GitHub `main`; avoid persisted stale
   markers because separate request handlers can observe different heads and
   complete out of order.
2. Filter different-commit pending rows from Rust approval blockers while
   retaining same-commit and queued/dispatching/unknown/executing blockers.
3. Apply the same rule to worker `claim`, `approve-and-queue`, and `next`.
4. Make the overview count only pending requests whose commit matches a
   verified current main revision.
5. Separate queued/running, uncertain, and failed-dispatch workbench groups;
   keep stale requests out of actionable approvals and avoid mislabeling them
   as operation-history entries.
6. Verify Rust and Python worker regressions, the embedded JavaScript, and
   source diff before integration.
