//! MathCraft document model (schema 2).
//!
//! Schema 1 stored a single `expression` string. Schema 2 keeps that field for
//! compatibility but adds sheets, named matrices and imported point series.
//! [`Document::normalize`] migrates a schema 1 payload in place; no separate
//! migration file is needed because every new field has a serde default.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::engine::matrix::Matrix;

fn ensure_finite<E: serde::de::Error>(value: f64) -> Result<f64, E> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(serde::de::Error::custom("number must be finite"))
    }
}

fn deserialize_finite_option<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<FiniteF64>::deserialize(deserializer).map(|value| value.map(|value| value.0))
}

fn serialize_finite_option<S>(value: &Option<f64>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match value {
        Some(value) => serializer.serialize_some(&FiniteValue(value)),
        None => serializer.serialize_none(),
    }
}

fn deserialize_finite_pairs<'de, D>(deserializer: D) -> Result<Vec<(String, f64)>, D::Error>
where
    D: Deserializer<'de>,
{
    Vec::<(String, FiniteF64)>::deserialize(deserializer).map(|values| {
        values
            .into_iter()
            .map(|(name, value)| (name, value.0))
            .collect()
    })
}

fn serialize_finite_pairs<S>(values: &[(String, f64)], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    if let Some((_, value)) = values.iter().find(|(_, value)| !value.is_finite()) {
        return Err(serde::ser::Error::custom(format!(
            "number '{value}' must be finite"
        )));
    }
    values.serialize(serializer)
}

fn deserialize_finite_coordinates<'de, D>(deserializer: D) -> Result<Vec<(f64, f64)>, D::Error>
where
    D: Deserializer<'de>,
{
    Vec::<(FiniteF64, FiniteF64)>::deserialize(deserializer)
        .map(|values| values.into_iter().map(|(x, y)| (x.0, y.0)).collect())
}

fn serialize_finite_coordinates<S>(values: &[(f64, f64)], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    if let Some((x, y)) = values
        .iter()
        .find(|(x, y)| !x.is_finite() || !y.is_finite())
    {
        return Err(serde::ser::Error::custom(format!(
            "point '{x}, {y}' must be finite"
        )));
    }
    values.serialize(serializer)
}

struct FiniteF64(f64);

struct FiniteValue<'a>(&'a f64);

impl Serialize for FiniteValue<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if self.0.is_finite() {
            self.0.serialize(serializer)
        } else {
            Err(serde::ser::Error::custom("number must be finite"))
        }
    }
}

impl<'de> Deserialize<'de> for FiniteF64 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        f64::deserialize(deserializer).and_then(|value| {
            ensure_finite(value)?;
            Ok(Self(value))
        })
    }
}

/// Project schema written by this build.
pub const SCHEMA_VERSION: u32 = 2;

/// One calculated expression and its persisted outcome.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cell {
    /// Expression as typed by the user.
    pub expression: String,
    /// Last successful numeric result.
    #[serde(
        default,
        serialize_with = "serialize_finite_option",
        deserialize_with = "deserialize_finite_option"
    )]
    pub result: Option<f64>,
    /// Last error message, when evaluation failed.
    #[serde(default)]
    pub error: Option<String>,
}

impl Cell {
    /// Build an empty cell.
    #[must_use]
    pub fn new(expression: impl Into<String>) -> Self {
        Self {
            expression: expression.into(),
            result: None,
            error: None,
        }
    }

    /// Canonical name of a cell by its zero-based index (`A1`, `A2`, ...).
    #[must_use]
    pub fn name(index: usize) -> String {
        format!("A{}", index + 1)
    }
}

/// A worksheet: a named list of expressions that may reference each other.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sheet {
    /// Sheet name, also usable as a variable in other sheets.
    #[serde(default)]
    pub name: String,
    /// Expressions and their persisted results.
    #[serde(default)]
    pub cells: Vec<Cell>,
}

impl Sheet {
    /// Build a sheet with one empty cell.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            cells: vec![Cell::new("")],
        }
    }
}

/// A matrix kept in the project under a name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedMatrix {
    /// User-visible name.
    #[serde(default)]
    pub name: String,
    /// Numeric coefficients, validated on load.
    pub matrix: Matrix,
}

/// A point series imported from CSV.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedSeries {
    /// User-visible name.
    #[serde(default)]
    pub name: String,
    /// Points in import order.
    #[serde(
        default,
        serialize_with = "serialize_finite_coordinates",
        deserialize_with = "deserialize_finite_coordinates"
    )]
    pub points: Vec<(f64, f64)>,
}

/// A saved MathCraft project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Document {
    /// Free-form document title.
    #[serde(default)]
    pub title: String,
    /// Legacy schema 1 expression, kept in sync with the active cell.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub expression: String,
    /// Named values available to every sheet.
    #[serde(
        default,
        serialize_with = "serialize_finite_pairs",
        deserialize_with = "deserialize_finite_pairs"
    )]
    pub variables: Vec<(String, f64)>,
    /// Expressions evaluated in this session, oldest first.
    #[serde(default)]
    pub history: Vec<String>,
    /// Worksheets of the notebook.
    #[serde(default)]
    pub sheets: Vec<Sheet>,
    /// Index of the visible sheet.
    #[serde(default)]
    pub active_sheet: usize,
    /// Matrices saved with the project.
    #[serde(default)]
    pub matrices: Vec<NamedMatrix>,
    /// CSV point series saved with the project.
    #[serde(default)]
    pub series: Vec<NamedSeries>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            title: "Untitled".to_owned(),
            expression: String::new(),
            variables: Vec::new(),
            history: Vec::new(),
            sheets: vec![Sheet::new("Sheet 1")],
            active_sheet: 0,
            matrices: Vec::new(),
            series: Vec::new(),
        }
    }
}

impl Document {
    /// Built-in demonstration content.
    #[must_use]
    pub fn sample() -> Self {
        Self {
            title: "MathCraft sample".to_owned(),
            expression: "sin(x) + 0.5*cos(2*x)".to_owned(),
            variables: vec![("a".to_owned(), 2.0), ("b".to_owned(), 3.0)],
            history: vec!["2+3*4".to_owned()],
            sheets: vec![
                Sheet {
                    name: "Plot".to_owned(),
                    cells: vec![
                        Cell::new("sin(x) + 0.5*cos(2*x)"),
                        Cell::new("a*x^2 + b"),
                        Cell::new("A1 + A2"),
                    ],
                },
                Sheet {
                    name: "Statistics".to_owned(),
                    cells: vec![Cell::new("mean(1,2,3,4)"), Cell::new("sqrt(16)")],
                },
            ],
            active_sheet: 0,
            matrices: vec![NamedMatrix {
                name: "A".to_owned(),
                matrix: Matrix::from_2x2(1.0, 2.0, 3.0, 4.0),
            }],
            series: Vec::new(),
        }
    }

    /// Migrate a schema 1 payload (or repair an incomplete schema 2 file).
    ///
    /// The legacy `expression` becomes the first cell of the first sheet. Once
    /// migrated, the legacy field is cleared so new saves are clean schema 2.
    pub fn normalize(&mut self) {
        if self.title.trim().is_empty() {
            self.title = "Untitled".to_owned();
        }
        if self.sheets.is_empty() {
            let expression = std::mem::take(&mut self.expression);
            self.sheets.push(Sheet {
                name: "Sheet 1".to_owned(),
                cells: vec![Cell::new(expression)],
            });
        }
        for sheet in &mut self.sheets {
            if sheet.cells.is_empty() {
                sheet.cells.push(Cell::new(""));
            }
        }
        if !self.expression.is_empty() {
            let legacy = std::mem::take(&mut self.expression);
            let already_present = self
                .sheets
                .iter()
                .flat_map(|sheet| &sheet.cells)
                .any(|cell| cell.expression == legacy);
            if !already_present {
                if let Some(cell) = self.sheets[0].cells.first_mut()
                    && cell.expression.is_empty()
                {
                    cell.expression = legacy;
                } else {
                    // An incomplete schema 2 file can contain both fields.
                    // Keep a distinct legacy expression instead of silently
                    // discarding it when the first cell is already populated.
                    self.sheets[0].cells.push(Cell::new(legacy));
                }
            }
        }
        let mut used_names: Vec<String> = Vec::new();
        let mut candidate_number = 1;
        for sheet in &mut self.sheets {
            sheet.name = sheet.name.trim().to_owned();
            if sheet.name.is_empty()
                || used_names
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(&sheet.name))
            {
                loop {
                    let candidate = format!("Sheet {candidate_number}");
                    candidate_number += 1;
                    if !used_names
                        .iter()
                        .any(|name| name.eq_ignore_ascii_case(&candidate))
                    {
                        sheet.name = candidate;
                        break;
                    }
                }
            }
            used_names.push(sheet.name.clone());
        }
        if self.active_sheet >= self.sheets.len() {
            self.active_sheet = self.sheets.len().saturating_sub(1);
        }
    }

    /// The currently selected sheet.
    #[must_use]
    pub fn active_sheet(&self) -> Option<&Sheet> {
        self.sheets.get(self.active_sheet)
    }

    /// The currently selected sheet, mutably.
    pub fn active_sheet_mut(&mut self) -> Option<&mut Sheet> {
        self.sheets.get_mut(self.active_sheet)
    }

    /// Look up a variable by exact name.
    #[must_use]
    pub fn variable(&self, name: &str) -> Option<f64> {
        self.variables
            .iter()
            .rev()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, value)| *value)
    }

    /// Mirror the active expression into the legacy field for schema 1 readers.
    pub fn sync_legacy_expression(&mut self) {
        self.expression = self
            .active_sheet()
            .and_then(|sheet| sheet.cells.first())
            .map(|cell| cell.expression.clone())
            .unwrap_or_default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use craftkit_core::{History, ProjectEnvelope};

    const SCHEMA_1_JSON: &str = r#"{
        "format": "mathcraft",
        "schema_version": 1,
        "app_version": "0.1.0",
        "document": {
            "title": "Old notebook",
            "expression": "2+3*4",
            "variables": [["a", 2.0]],
            "history": ["2+3*4"]
        }
    }"#;

    #[test]
    fn sample_has_sheets_variables_and_a_matrix() {
        let document = Document::sample();
        assert!(document.sheets.len() >= 2);
        assert_eq!(document.variables.len(), 2);
        assert_eq!(document.variable("a"), Some(2.0));
        assert_eq!(document.matrices.len(), 1);
    }

    #[test]
    fn schema_1_migrates_to_schema_2() {
        let envelope: ProjectEnvelope<Document> = serde_json::from_str(SCHEMA_1_JSON).unwrap();
        envelope.validate("mathcraft", SCHEMA_VERSION).unwrap();
        let mut document = envelope.document;
        assert!(document.sheets.is_empty());
        document.normalize();
        assert_eq!(document.sheets.len(), 1);
        assert_eq!(document.sheets[0].cells[0].expression, "2+3*4");
        assert_eq!(document.variable("a"), Some(2.0));
        assert_eq!(document.history, vec!["2+3*4".to_owned()]);
        assert!(document.matrices.is_empty());
        assert!(document.expression.is_empty());
        assert_eq!(document.active_sheet, 0);
    }

    #[test]
    fn schema_2_round_trip_keeps_sheets_matrices_and_series() {
        let mut document = Document::sample();
        document.series.push(NamedSeries {
            name: "data".to_owned(),
            points: vec![(0.0, 1.0), (1.0, 2.0)],
        });
        let envelope = ProjectEnvelope::new("mathcraft", SCHEMA_VERSION, "0.2.0", document.clone());
        let json = serde_json::to_string(&envelope).unwrap();
        let restored: ProjectEnvelope<Document> = serde_json::from_str(&json).unwrap();
        restored.validate("mathcraft", SCHEMA_VERSION).unwrap();
        assert_eq!(restored.document, document);
        assert_eq!(restored.document.sheets[0].cells[1].expression, "a*x^2 + b");
        assert_eq!(restored.document.series[0].points.len(), 2);
    }

    #[test]
    fn malformed_matrix_is_rejected_on_load() {
        let json = r#"{
            "format": "mathcraft", "schema_version": 2, "app_version": "0.2.0",
            "document": {
                "title": "Bad", "expression": "", "variables": [], "history": [],
                "sheets": [], "active_sheet": 0,
                "matrices": [{"name": "A", "matrix": {"rows": [[1.0],[2.0,3.0]]}}],
                "series": []
            }
        }"#;
        assert!(serde_json::from_str::<ProjectEnvelope<Document>>(json).is_err());
    }

    #[test]
    fn undo_restores_sheet_edits() {
        let mut history = History::new(Document::default());
        history.apply(|document| {
            document.active_sheet_mut().unwrap().cells[0].expression = "1+1".to_owned();
        });
        assert_eq!(
            history.current().active_sheet().unwrap().cells[0].expression,
            "1+1"
        );
        assert!(history.undo());
        assert_eq!(
            history.current().active_sheet().unwrap().cells[0].expression,
            ""
        );
        assert!(history.redo());
        assert_eq!(
            history.current().active_sheet().unwrap().cells[0].expression,
            "1+1"
        );
    }

    #[test]
    fn incomplete_documents_are_normalized_without_losing_the_legacy_expression() {
        let json = r#"{
            "format": "mathcraft", "schema_version": 2, "app_version": "0.2.2",
            "document": {
                "expression": "2+2",
                "sheets": [{"name": "", "cells": [{"expression": "1+1"}]}],
                "active_sheet": 99
            }
        }"#;
        let mut envelope: ProjectEnvelope<Document> = serde_json::from_str(json).unwrap();
        envelope.document.normalize();
        let document = envelope.document;
        assert_eq!(document.title, "Untitled");
        assert_eq!(document.sheets[0].name, "Sheet 1");
        assert_eq!(document.sheets[0].cells.len(), 2);
        assert_eq!(document.sheets[0].cells[1].expression, "2+2");
        assert_eq!(document.active_sheet, 0);
        assert!(document.expression.is_empty());
    }

    #[test]
    fn editing_gesture_keeps_the_first_keystroke_in_one_undo_step() {
        let mut history = History::new(Document::default());
        history.begin_gesture();
        if let Some(cell) = history
            .current_mut()
            .active_sheet_mut()
            .and_then(|sheet| sheet.cells.first_mut())
        {
            cell.expression = "1".to_owned();
        }
        if let Some(cell) = history
            .current_mut()
            .active_sheet_mut()
            .and_then(|sheet| sheet.cells.first_mut())
        {
            cell.expression = "1+1".to_owned();
        }
        history.end_gesture();
        assert_eq!(history.undo_depth(), 1);
        assert!(history.undo());
        assert!(
            history
                .current()
                .active_sheet()
                .and_then(|sheet| sheet.cells.first())
                .is_some_and(|cell| cell.expression.is_empty())
        );
    }

    #[test]
    fn schema_2_legacy_mirror_does_not_create_duplicate_cells_on_open() {
        let mut document = Document::sample();
        document.active_sheet = 1;
        document.sync_legacy_expression();
        let mirrored = document.expression.clone();
        document.normalize();
        assert_eq!(document.sheets[0].cells.len(), 3);
        assert_eq!(document.sheets[1].cells.len(), 2);
        assert_eq!(document.sheets[1].cells[0].expression, mirrored);
        assert!(document.expression.is_empty());
        document.normalize();
        assert_eq!(document.sheets[0].cells.len(), 3);
    }

    #[test]
    fn duplicate_sheet_names_are_repaired_case_insensitively() {
        let mut document = Document::default();
        document.sheets.push(Sheet::new("sheet 1"));
        document.sheets.push(Sheet::new("   "));
        document.normalize();
        assert_eq!(document.sheets[0].name, "Sheet 1");
        assert_eq!(document.sheets[1].name, "Sheet 2");
        assert_eq!(document.sheets[2].name, "Sheet 3");
    }

    #[test]
    fn variable_lookup_matches_evaluator_precedence() {
        let document = Document {
            variables: vec![("a".to_owned(), 1.0), ("a".to_owned(), 2.0)],
            ..Document::default()
        };
        assert_eq!(document.variable("a"), Some(2.0));
    }

    #[test]
    fn serialization_rejects_non_finite_persisted_numbers() {
        let mut document = Document::default();
        document.variables.push(("bad".to_owned(), f64::NAN));
        assert!(serde_json::to_string(&document).is_err());

        let mut document = Document::default();
        document.series.push(NamedSeries {
            name: "bad".to_owned(),
            points: vec![(0.0, f64::INFINITY)],
        });
        assert!(serde_json::to_string(&document).is_err());

        let mut document = Document::default();
        document.sheets[0].cells[0].result = Some(f64::NEG_INFINITY);
        assert!(serde_json::to_string(&document).is_err());
    }
}
