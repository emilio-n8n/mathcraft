//! Worksheet evaluation with cell-to-cell references and clear diagnostics.

use crate::document::{Cell, Sheet};
use crate::engine::expr::{self, Expr, ExprKind, format_value};

/// The outcome of evaluating one cell.
#[derive(Debug, Clone, PartialEq)]
pub struct CellOutcome {
    /// Numeric result, present when evaluation succeeded.
    pub value: Option<f64>,
    /// Error message, present when evaluation failed.
    pub error: Option<String>,
}

impl CellOutcome {
    fn value(value: f64) -> Self {
        Self {
            value: Some(value),
            error: None,
        }
    }

    fn error(message: impl Into<String>) -> Self {
        Self {
            value: None,
            error: Some(message.into()),
        }
    }

    fn empty() -> Self {
        Self {
            value: None,
            error: None,
        }
    }
}

/// Evaluate every cell of a sheet.
///
/// Cells may reference other cells by `A1`, `A2`, ... in either case. Only
/// referenced cells are resolved recursively, so a cell that merely exists
/// cannot create a false cycle. Document variables are visible to every cell;
/// callers can inject other sheets' results as additional variables.
#[must_use]
pub fn evaluate_sheet(sheet: &Sheet, variables: &[(String, f64)]) -> Vec<CellOutcome> {
    let parsed: Vec<Result<Expr, expr::MathError>> = sheet
        .cells
        .iter()
        .map(|cell| {
            if cell.expression.trim().is_empty() {
                Err(expr::MathError::Syntax {
                    message: String::new(),
                    pos: None,
                })
            } else {
                expr::parse(&cell.expression)
            }
        })
        .collect();
    let mut cache = vec![None; sheet.cells.len()];
    let mut visiting = vec![false; sheet.cells.len()];
    (0..sheet.cells.len())
        .map(|index| evaluate_cell(index, sheet, &parsed, variables, &mut cache, &mut visiting))
        .collect()
}

fn collect_names(expression: &Expr, names: &mut Vec<String>) {
    match &expression.kind {
        ExprKind::Number(_) => {}
        ExprKind::Variable(name) => names.push(name.clone()),
        ExprKind::Unary { value, .. } => collect_names(value, names),
        ExprKind::Binary { left, right, .. } => {
            collect_names(left, names);
            collect_names(right, names);
        }
        ExprKind::Call { args, .. } => {
            for argument in args {
                collect_names(argument, names);
            }
        }
    }
}

fn evaluate_cell(
    index: usize,
    sheet: &Sheet,
    parsed: &[Result<Expr, expr::MathError>],
    variables: &[(String, f64)],
    cache: &mut [Option<f64>],
    visiting: &mut [bool],
) -> CellOutcome {
    if let Some(value) = cache[index] {
        return CellOutcome::value(value);
    }
    if sheet.cells[index].expression.trim().is_empty() {
        return CellOutcome::empty();
    }
    if visiting[index] {
        return CellOutcome::error(format!("circular reference in {}", Cell::name(index)));
    }
    let expression = match &parsed[index] {
        Ok(expression) => expression,
        Err(error) => return CellOutcome::error(error.to_string()),
    };

    let mut names = Vec::new();
    collect_names(expression, &mut names);
    visiting[index] = true;
    let mut scoped = variables.to_vec();
    for (other, source) in sheet.cells.iter().enumerate() {
        if other == index || source.expression.trim().is_empty() {
            continue;
        }
        let name = Cell::name(other);
        if !names.iter().any(|used| used.eq_ignore_ascii_case(&name)) {
            continue;
        }
        let outcome = evaluate_cell(other, sheet, parsed, variables, cache, visiting);
        if let Some(value) = outcome.value {
            scoped.push((name.clone(), value));
            scoped.push((name.to_lowercase(), value));
        } else if let Some(error) = outcome.error {
            visiting[index] = false;
            return CellOutcome::error(format!("{name}: {error}"));
        }
    }
    visiting[index] = false;

    match expr::eval(expression, &scoped) {
        Ok(value) if value.is_finite() => {
            cache[index] = Some(value);
            CellOutcome::value(value)
        }
        Ok(value) => CellOutcome::error(format!("non-finite result {}", format_value(value))),
        Err(error) => CellOutcome::error(error.to_string()),
    }
}

/// Copy the computed outcomes back into the sheet for persistence.
pub fn apply_outcomes(sheet: &mut Sheet, outcomes: &[CellOutcome]) {
    for (cell, outcome) in sheet.cells.iter_mut().zip(outcomes) {
        cell.result = outcome.value;
        cell.error = outcome.error.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sheet(expressions: &[&str]) -> Sheet {
        Sheet {
            name: "S".to_owned(),
            cells: expressions
                .iter()
                .map(|expression| Cell::new((*expression).to_owned()))
                .collect(),
        }
    }

    #[test]
    fn evaluates_cells_and_caches_references() {
        let document = sheet(&["2+3*4", "A1 * 2", "a1 + A2"]);
        let variables = vec![("a".to_owned(), std::f64::consts::PI)];
        let outcomes = evaluate_sheet(&document, &variables);
        assert_eq!(outcomes[0].value, Some(14.0));
        assert_eq!(outcomes[1].value, Some(28.0));
        assert_eq!(outcomes[2].value, Some(42.0));
    }

    #[test]
    fn reports_cycles_and_unknown_names() {
        let cyclic = sheet(&["A2 + 1", "A1 * 2"]);
        let outcomes = evaluate_sheet(&cyclic, &[]);
        assert!(
            outcomes[0]
                .error
                .as_deref()
                .is_some_and(|message| message.contains("circular reference"))
        );

        let unknown = sheet(&["missing + 1", "A1 + 1"]);
        let outcomes = evaluate_sheet(&unknown, &[]);
        assert!(
            outcomes[0]
                .error
                .as_deref()
                .is_some_and(|message| message.contains("unknown variable 'missing'"))
        );
        assert!(
            outcomes[1]
                .error
                .as_deref()
                .is_some_and(|message| message.contains("A1:"))
        );
    }

    #[test]
    fn persists_outcomes_back_to_cells() {
        let mut document = sheet(&["1/0", "sqrt(9)"]);
        let outcomes = evaluate_sheet(&document, &[]);
        apply_outcomes(&mut document, &outcomes);
        assert!(
            document.cells[0]
                .error
                .as_deref()
                .is_some_and(|error| !error.is_empty())
        );
        assert_eq!(document.cells[1].result, Some(3.0));
        assert_eq!(document.cells[1].error, None);
    }
}
