# MathCraft

Scientific calculator, matrices and 2D plots

**MathCraft is an independent, community open-source project.** It is not an
official product of, endorsed by, or affiliated with any third-party vendor it
may interoperate with.

Built on [CraftKit](https://github.com/emilio-n8n/craftkit), a small native Rust
desktop foundation.

## Features

- Native Rust desktop UI (egui/eframe), no web runtime.
- Notebook with multiple named sheets, each holding several expressions.
  Cell results and errors are persisted in the project. Cells can reference
  each other (`A1`, `A2`, case-insensitive), another sheet (`Plot.A1`) or a
  quoted sheet name (`'My Sheet'.A1`). Cross-sheet values are recomputed before
  evaluation, saving and export; self- and cross-sheet circular references are
  reported explicitly. Sheet names can be edited; update qualified references
  manually after a rename.
- Expression parser and evaluator: `+ - * / ^`, parentheses, implicit
  multiplication (`2x`, `2(x+1)`), scientific notation (`1.5e-3`), variables,
  constants `pi` and `e`, and 22 functions (trigonometry and inverse
  trigonometry, logarithms, rounding, `min`/`max`/`sum`/`mean`, ...).
  Syntax errors include a source excerpt with a one-based line and column.
- Named variables with add/remove editing and a bounded, clickable history of
  evaluated expressions.
- Dense matrix arithmetic (addition, subtraction, multiplication, transpose,
  determinant) with named matrices saved in the project and loadable into
  either operand.
- Interactive 2D plot of `y = f(x)`: drag to pan, pointer-anchored scroll zoom,
  editable X/Y ranges, several curves at once, imported CSV series. Export to
  CSV or PNG; PNGs include legends and labelled graduations without a plotting
  dependency.
- CSV import of `x,y,z,...` point series (one series per Y column), including
  quoted and multiline headers, plus CSV export of results and variables.
- Versioned JSON project files (schema 2) with atomic saves and automatic
  migration of schema 1 notebooks.
- Bounded undo/redo through the shared document history.
- Recent files and persisted preferences.
- Offline-first: no account, no telemetry, no network required.

## Run

```bash
cargo run --release -- --sample
cargo run --release -- --open project.json
```

Without arguments the application opens an empty document.

- `--sample` opens the built-in demonstration document.
- `--open <file>` opens a project.
- `--version` prints the version.
- `--help` prints usage.

## Build and test

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Linux windowing development packages are installed by the CI workflow.

## Project layout

```text
src/main.rs         CLI and window startup
src/app.rs          UI, menus and lifecycle
src/lib.rs          UI-free library surface
src/document.rs     schema 2 document model and migration
src/engine/expr.rs  tokenizer, parser and evaluator
src/engine/matrix.rs matrix arithmetic and persistence
src/engine/sheet.rs worksheet evaluation and cell references
src/engine/plot.rs  pure plot sampling helpers
src/engine/csv.rs   CSV import/export helpers
src/engine/raster.rs PNG raster and plot rendering
```

The application depends on pinned CraftKit crates from Git. Keep the tag in
`Cargo.toml` and `Cargo.lock` consistent when upgrading the foundation.

## File format

Projects are JSON envelopes:

```json
{
  "format": "mathcraft",
  "schema_version": 2,
  "app_version": "0.1.0",
  "document": { }
}
```

The document holds `sheets` (expressions and persisted results), `variables`,
`active_sheet`, named `matrices`, imported point `series` and `history`. The
legacy schema 1 `expression` is migrated into the first sheet on open; a schema
2 legacy mirror is recognised without duplicating cells. A newer
`schema_version` is rejected instead of being silently misread. Persisted
numbers must be finite, and malformed matrices or CSV series are rejected.
The launcher descriptor `craft-app.json` is independent of the document schema
and stays at schema 1.

## Licence

Licensed under either of:

- Apache License, Version 2.0 (`LICENSE-APACHE`)
- MIT license (`LICENSE-MIT`)

at your option.

## Third-party notices

See [THIRD_PARTY.md](THIRD_PARTY.md).
