# Changelog — MathCraft

All notable changes to MathCraft are documented here.

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
