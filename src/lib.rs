//! MathCraft's UI-free core: document schema and domain algorithms.
//!
//! The binary crate owns the egui application and reuses these modules, which
//! keeps parsing, matrix math and plot sampling testable without a window.

pub mod document;
pub mod engine;
