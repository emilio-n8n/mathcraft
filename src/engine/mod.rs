//! UI-free MathCraft domain algorithms.
//!
//! Everything testable without egui lives here: expression parsing, matrix
//! arithmetic, plot sampling, worksheet evaluation and CSV import/export.
//! Raster image encoding for PNG export lives in [`raster`].

pub mod csv;
pub mod expr;
pub mod matrix;
pub mod plot;
pub mod raster;
pub mod sheet;

pub use expr::format_value;
