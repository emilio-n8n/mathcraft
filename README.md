# MathCraft

Scientific calculator, matrices and 2D plots

**MathCraft is an independent, community open-source project.** It is not an
official product of, endorsed by, or affiliated with any third-party vendor it
may interoperate with.

Built on [CraftKit](https://github.com/emilio-n8n/craftkit), a small native Rust
desktop foundation.

## Features

- Native Rust desktop UI (egui/eframe), no web runtime.
- Expression parser and evaluator: `+ - * / ^`, parentheses, variables,
  constants `pi` and `e`, and functions `sin cos tan sqrt abs ln log exp`.
  Syntax errors report the offending token and its byte position.
- Document variables with add/remove editing, plus a bounded history of
  evaluated expressions.
- Dense matrix arithmetic: addition, subtraction, multiplication by another
  matrix or a scalar, transpose and determinant.
- 2D plot of `y = f(x)` drawn directly on the egui `Painter`: axes, grid,
  graduations and curve, with no plotting dependency. The variable `x` is
  sampled over a configurable interval.
- Versioned JSON project files with atomic saves.
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
src/document.rs     application document model
src/engine/expr.rs  tokenizer, parser and evaluator
src/engine/matrix.rs matrix arithmetic
src/engine/plot.rs  pure plot sampling helpers
```

The application depends on pinned CraftKit crates from Git. Keep the tag in
`Cargo.toml` and `Cargo.lock` consistent when upgrading the foundation.

## File format

Projects are JSON envelopes:

```json
{
  "format": "mathcraft",
  "schema_version": 1,
  "app_version": "0.1.0",
  "document": { }
}
```

The application owns the `document` schema and its migrations. A newer
`schema_version` is rejected instead of being silently misread.

## Licence

Licensed under either of:

- Apache License, Version 2.0 (`LICENSE-APACHE`)
- MIT license (`LICENSE-MIT`)

at your option.

## Third-party notices

See [THIRD_PARTY.md](THIRD_PARTY.md).
