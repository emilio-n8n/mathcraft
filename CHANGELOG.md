# Changelog — MathCraft

All notable changes to MathCraft are documented here.

## [Unreleased]

### Fixed

- Expressions now support qualified and quoted worksheet references, report
  line/column diagnostics, reject non-finite results and bound pathological
  nesting without exhausting the stack.
- Worksheet evaluation detects self- and cross-sheet cycles, resolves fresh
  cross-sheet values and reports references to empty cells explicitly.
- CSV import validates quoting, embedded newlines, finite numbers and column
  counts instead of silently discarding malformed first rows.
- Plot CSV export aligns the union of series abscissae and preserves numeric
  precision; matrix determinants no longer treat small non-zero pivots as zero.
- PNG line rendering clips extreme coordinates before rasterisation, and plot
  pan/zoom handles invalid or overflowing ranges safely.
- Saving refreshes all worksheet results, opening recomputes stale outcomes,
  and schema 2 legacy mirrors no longer create duplicate cells during migrate.
- Editing gestures snapshot before the first keystroke; consecutive duplicate
  history entries are suppressed.

### Added

- Multi-column CSV import (`x,y,z,...`) creates one named series per Y column.
- PNG exports include curve legends and readable axis graduations.
- Matrix addition and subtraction, multiline matrix input, and loading saved
  matrices into either operand.
- Worksheet tabs scroll, names can be edited with duplicate checks, and
  deleting a sheet now selects the correct neighbour.
- Defensive finite-number validation for persisted variables, cell results,
  matrices and imported series.

## [0.2.2] - 2026-11-23

### Fixed

- The CSV import panel moved to the scrollable left panel, so it is reachable
  on windows where the central notebook content extends below the viewport.

## [0.2.1] - 2026-11-23

### Fixed

- Opening a notebook keeps its file path: the title shows the file name and
  Save writes in place instead of asking for a new destination.

## [0.2.0] - 2026-11-22

### Changed

- Document schema 2: notebooks with multiple sheets, persisted cell results,
  named matrices and CSV series. Schema 1 files are migrated on open.

### Added

- Expression parser with scientific notation, implicit multiplication and 22
  functions including inverse trigonometry, rounding and basic statistics.
- Cell-to-cell references with cycle detection and detailed error messages.
- Interactive plot: drag to pan, scroll to zoom, editable ranges, several
  curves, CSV import, and CSV/PNG export via a small raster encoder.
- CSV import/export for point series, results and variables.
- Examples menu and collapsible syntax help.
- Tests for migration, parser, matrices, sheets, plot, CSV and undo.

## [0.1.0] - 2026-11-22

### Added

- Initial CraftKit-based application shell.
- Versioned project files with atomic save.
- Undo/redo through the shared document history.
- Background jobs with progress and cancellation.
- Application-specific document model and UI.
