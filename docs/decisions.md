# Decisions — MathCraft

Short architecture decision records. Newest first.

## ADR-001 — Built on CraftKit

Date: 2026-11-22

Decision: use the pinned CraftKit crates for the native shell, project
lifecycle, history and background jobs.

Alternatives: build the desktop foundation from scratch; use another UI
framework.

Reasons: consistent behaviour across the portfolio, fewer duplicated bugs,
shared release and testing conventions.

Consequences: CraftKit upgrades are explicit tag changes; domain code stays in
this repository.

## ADR-002 — Initial document model

Date: 2026-11-22

Decision: the document stores a title, one expression, a list of
`(name, value)` variables and a bounded list of evaluated expressions. Schema
version stays at 1 for this first domain revision.

Alternatives: separate documents per worksheet; storing only the expression
history; storing matrices in the document.

Reasons: one expression plus its variables is enough to reproduce the current
calculation and plot after reopening a project. Matrices are entered in a
scratch panel and are intentionally not persisted yet.

Consequences: adding persisted worksheets or matrices requires a schema bump
and a migration test. The plot always binds `x` to the sampled abscissa,
overriding any document variable named `x`.
