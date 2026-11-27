//! Worksheet evaluation with cell-to-cell references and clear diagnostics.

use crate::document::{Cell, Sheet};
use crate::engine::expr::{self, Expr, ExprKind};

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
/// Cells may reference other cells by `A1`, `A2`, ... in either case. Qualified
/// references such as `Plot.A1` and `'My Sheet'.A1` are also accepted. Only
/// referenced cells are resolved recursively, so a cell that merely exists
/// cannot create a false cycle. Document variables are visible to every cell;
/// callers can inject other sheets' results as additional variables.
#[must_use]
pub fn evaluate_sheet(sheet: &Sheet, variables: &[(String, f64)]) -> Vec<CellOutcome> {
    evaluate_workbook(std::slice::from_ref(sheet), variables)
        .into_iter()
        .next()
        .unwrap_or_default()
}

/// Evaluate all worksheets together, resolving cross-sheet dependencies lazily.
///
/// The returned outcomes have the same shape as `sheets`. A sheet name used as
/// a variable resolves to its first non-empty successful cell, matching the
/// notebook's historical scope rules. Qualified cell references always use the
/// fresh result from this evaluation rather than a persisted, potentially stale
/// value.
#[must_use]
pub fn evaluate_workbook(sheets: &[Sheet], variables: &[(String, f64)]) -> Vec<Vec<CellOutcome>> {
    let parsed = sheets
        .iter()
        .map(|sheet| {
            sheet
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
                .collect()
        })
        .collect();
    let cache = sheets
        .iter()
        .map(|sheet| vec![None; sheet.cells.len()])
        .collect();
    let mut evaluator = WorkbookEvaluator {
        sheets,
        parsed,
        variables,
        cache,
        stack: Vec::new(),
    };
    for (sheet, worksheet) in sheets.iter().enumerate() {
        for cell in 0..worksheet.cells.len() {
            evaluator.evaluate(sheet, cell);
        }
    }
    evaluator
        .cache
        .into_iter()
        .map(|sheet| {
            sheet
                .into_iter()
                .map(|outcome| outcome.unwrap_or_else(CellOutcome::empty))
                .collect()
        })
        .collect()
}

struct WorkbookEvaluator<'a> {
    sheets: &'a [Sheet],
    parsed: Vec<Vec<Result<Expr, expr::MathError>>>,
    variables: &'a [(String, f64)],
    cache: Vec<Vec<Option<CellOutcome>>>,
    stack: Vec<(usize, usize)>,
}

impl WorkbookEvaluator<'_> {
    fn evaluate(&mut self, sheet_index: usize, cell_index: usize) -> CellOutcome {
        if let Some(outcome) = self.cache[sheet_index][cell_index].clone() {
            return outcome;
        }
        let expression = &self.sheets[sheet_index].cells[cell_index].expression;
        if expression.trim().is_empty() {
            let outcome = CellOutcome::empty();
            self.cache[sheet_index][cell_index] = Some(outcome.clone());
            return outcome;
        }
        if let Some(cycle_start) = self
            .stack
            .iter()
            .position(|&(sheet, cell)| sheet == sheet_index && cell == cell_index)
        {
            let mut chain = self.stack[cycle_start..]
                .iter()
                .map(|&(sheet, cell)| self.cell_label(sheet, cell))
                .collect::<Vec<_>>();
            chain.push(self.cell_label(sheet_index, cell_index));
            return CellOutcome::error(format!("circular reference: {}", chain.join(" -> ")));
        }
        if self.stack.len() >= expr::MAX_PARSE_DEPTH {
            return CellOutcome::error(format!(
                "cell dependency nesting is too deep (maximum {})",
                expr::MAX_PARSE_DEPTH
            ));
        }

        self.stack.push((sheet_index, cell_index));
        let source = self.sheets[sheet_index].cells[cell_index]
            .expression
            .clone();
        let outcome = match self.parsed[sheet_index][cell_index].clone() {
            Ok(expression) => self.evaluate_parsed(sheet_index, cell_index, &expression),
            Err(error) => CellOutcome::error(error.diagnostic(&source)),
        };
        self.stack.pop();
        self.cache[sheet_index][cell_index] = Some(outcome.clone());
        outcome
    }

    fn evaluate_parsed(
        &mut self,
        sheet_index: usize,
        cell_index: usize,
        expression: &Expr,
    ) -> CellOutcome {
        let mut names = Vec::new();
        collect_names(expression, &mut names);
        names.sort();
        names.dedup();
        let mut scoped = self.variables.to_vec();

        for name in &names {
            if let Some((target_sheet, target_cell)) = self.resolve_reference(sheet_index, name) {
                let label = self.cell_label(target_sheet, target_cell);
                let Some(target) = self.sheets[target_sheet].cells.get(target_cell) else {
                    return CellOutcome::error(format!(
                        "{label} is outside the sheet ({} cells)",
                        self.sheets[target_sheet].cells.len()
                    ));
                };
                if target.expression.trim().is_empty() {
                    return CellOutcome::error(format!("{label} is empty"));
                }
                let outcome = self.evaluate(target_sheet, target_cell);
                if let Some(value) = outcome.value {
                    // Preserve the spelling used by the expression: evaluation
                    // of ordinary variables is intentionally case-sensitive.
                    scoped.push((name.clone(), value));
                } else if let Some(error) = outcome.error {
                    return CellOutcome::error(format!("{label}: {error}"));
                } else {
                    return CellOutcome::error(format!("{label} has no value"));
                }
                continue;
            }

            if let Some(target_sheet) = self
                .sheets
                .iter()
                .position(|sheet| sheet.name.eq_ignore_ascii_case(name))
            {
                let mut first_error = None;
                let mut resolved = false;
                for target_cell in 0..self.sheets[target_sheet].cells.len() {
                    if self.sheets[target_sheet].cells[target_cell]
                        .expression
                        .trim()
                        .is_empty()
                    {
                        continue;
                    }
                    let outcome = self.evaluate(target_sheet, target_cell);
                    if let Some(value) = outcome.value {
                        scoped.push((name.clone(), value));
                        resolved = true;
                        break;
                    }
                    if first_error.is_none()
                        || outcome
                            .error
                            .as_deref()
                            .is_some_and(|error| error.contains("circular reference"))
                    {
                        first_error = outcome.error;
                    }
                }
                if !resolved {
                    let detail = first_error.map_or_else(
                        || "has no calculated cells".to_owned(),
                        |error| format!(": {error}"),
                    );
                    return CellOutcome::error(format!("sheet '{name}'{detail}"));
                }
            }
        }

        match expr::eval(expression, &scoped) {
            Ok(value) => CellOutcome::value(value),
            Err(error) => CellOutcome::error(
                error.diagnostic(&self.sheets[sheet_index].cells[cell_index].expression),
            ),
        }
    }

    fn resolve_reference(&self, sheet_index: usize, name: &str) -> Option<(usize, usize)> {
        let (qualifier, cell_name) = name
            .rsplit_once('.')
            .map_or((None, name), |(sheet, cell)| (Some(sheet), cell));
        let cell_index = parse_cell_index(cell_name)?;
        let target_sheet = if let Some(qualifier) = qualifier {
            self.sheets
                .iter()
                .position(|sheet| sheet.name.eq_ignore_ascii_case(qualifier))?
        } else {
            sheet_index
        };
        Some((target_sheet, cell_index))
    }

    fn cell_label(&self, sheet_index: usize, cell_index: usize) -> String {
        format!(
            "{}.{}",
            self.sheets[sheet_index].name,
            Cell::name(cell_index)
        )
    }
}

fn parse_cell_index(name: &str) -> Option<usize> {
    let bytes = name.as_bytes();
    if bytes.len() < 2 || !bytes[0].eq_ignore_ascii_case(&b'A') {
        return None;
    }
    if !bytes[1..].iter().all(u8::is_ascii_digit) {
        return None;
    }
    let number = name[1..].parse::<usize>().ok()?;
    if number == 0 {
        return None;
    }
    number.checked_sub(1)
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
                .is_some_and(|message| message.contains("S.A1:"))
        );
    }

    #[test]
    fn direct_self_references_report_the_cycle_chain() {
        let outcomes = evaluate_sheet(&sheet(&["A1 + 1", "1"]), &[]);
        let error = outcomes[0].error.as_deref().unwrap_or_default();
        assert!(
            error.contains("circular reference: S.A1 -> S.A1"),
            "{error}"
        );
        assert_eq!(outcomes[1].value, Some(1.0));
    }

    #[test]
    fn qualified_references_work_across_sheets_and_detect_cross_cycles() {
        let first = sheet(&["2", "A1 * 3"]);
        let mut second = Sheet::new("Other sheet");
        second.cells = vec![Cell::new("'S'.A2 + 1"), Cell::new("S.A1 + 1")];
        let outcomes = evaluate_workbook(&[first, second], &[]);
        assert_eq!(outcomes[0][1].value, Some(6.0));
        assert_eq!(outcomes[1][0].value, Some(7.0));
        assert_eq!(outcomes[1][1].value, Some(3.0));

        let mut first = sheet(&["'Other sheet'.A1"]);
        first.name = "First".to_owned();
        let mut second = Sheet::new("Other sheet");
        second.cells = vec![Cell::new("First.A1 + 1")];
        let outcomes = evaluate_workbook(&[first, second], &[]);
        let error = outcomes[0][0].error.as_deref().unwrap_or_default();
        assert!(error.contains("circular reference"), "{error}");
        assert!(error.contains("First.A1"), "{error}");
        assert!(error.contains("Other sheet.A1"), "{error}");
    }

    #[test]
    fn local_cells_shadow_external_variables_and_empty_refs_are_explicit() {
        let outcomes = evaluate_sheet(
            &sheet(&["5", "a1 + A1", "A4", ""]),
            &[("A1".to_owned(), 100.0)],
        );
        assert_eq!(outcomes[1].value, Some(10.0));
        assert!(
            outcomes[2]
                .error
                .as_deref()
                .is_some_and(|error| error.contains("S.A4 is empty"))
        );

        let outcomes = evaluate_sheet(&sheet(&["A999999", "1"]), &[]);
        assert!(
            outcomes[0]
                .error
                .as_deref()
                .is_some_and(|error| error.contains("S.A999999 is outside the sheet"))
        );
        assert_eq!(outcomes[1].value, Some(1.0));
    }

    #[test]
    fn sheet_variables_use_the_first_successful_cell() {
        let mut source = sheet(&["1/0", "4", "99"]);
        source.name = "Data".to_owned();
        let mut target = Sheet::new("Result");
        target.cells = vec![Cell::new("data * 2")];
        let outcomes = evaluate_workbook(&[source, target], &[]);
        assert_eq!(outcomes[1][0].value, Some(8.0));
    }

    #[test]
    fn dependency_depth_is_bounded_without_a_stack_overflow() {
        let expressions = (0..300)
            .map(|index| {
                if index + 1 == 300 {
                    "1".to_owned()
                } else {
                    format!("A{} + 1", index + 2)
                }
            })
            .collect::<Vec<_>>();
        let references = expressions.iter().map(String::as_str).collect::<Vec<_>>();
        let outcomes = evaluate_sheet(&sheet(&references), &[]);
        assert!(
            outcomes[0]
                .error
                .as_deref()
                .is_some_and(|error| error.contains("nesting is too deep"))
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
