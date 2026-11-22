//! UI-free MathCraft domain algorithms.
//!
//! Anything that can be tested without egui lives here: expression parsing and
//! evaluation, matrix arithmetic and plot sampling.

pub mod expr;
pub mod matrix;
pub mod plot;

pub use expr::format_value;
