# Decisions — MathCraft

Short architecture decision records. Newest first.

## ADR-003 — Schema 2 notebook

Date: 2026-11-22

Decision: schema 2 stores an array of sheets with expressions and persisted
results, named matrices, imported series, variables and the active sheet. The
legacy `expression` field is retained with a serde default and migrated into
the first sheet by `Document::normalize()`.

Alternatives: keep schema 1 and encode sheets inside a single string; bump the
launcher manifest schema; require users to recreate old documents.

Reasons: v0.1.0 files must keep opening after the v0.2.0 upgrade. CraftKit
accepts older envelopes, so in-place normalisation on load is sufficient;
saving writes schema 2. The launcher descriptor is release metadata and is
unrelated to the document payload, so it stays at schema 1.

Consequences: every new field needs a serde default and a malformed payload
must be rejected (matrix rows are validated on deserialisation). A future
incompatible change requires schema 3 and a new migration test.

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

Consequences: superseded by ADR-003: worksheets and matrices are now persisted
under schema 2 with a migration test. The plot still binds `x` to the sampled
abscissa, overriding any document variable named `x`.

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
