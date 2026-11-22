//! Dependency-free CSV helpers for point series and tabular exports.

use std::fmt;

use crate::engine::expr::format_value;
use crate::engine::plot::Sample;

/// A CSV import error with a one-based line number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CsvError {
    /// A row does not have the expected number of columns.
    WrongColumns {
        /// One-based line number in the source text.
        line: usize,
        /// Expected number of columns.
        expected: usize,
        /// Actual number of columns.
        found: usize,
    },
    /// A numeric field could not be parsed.
    InvalidNumber {
        /// One-based line number in the source text.
        line: usize,
        /// One-based column number.
        column: usize,
        /// Offending text.
        value: String,
    },
}

impl fmt::Display for CsvError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongColumns {
                line,
                expected,
                found,
            } => write!(
                formatter,
                "line {line}: expected {expected} columns, found {found}"
            ),
            Self::InvalidNumber {
                line,
                column,
                value,
            } => write!(
                formatter,
                "line {line}, column {column}: invalid number '{value}'"
            ),
        }
    }
}

impl std::error::Error for CsvError {}

/// Parse a two-column `x,y` series, ignoring blank lines and one header row.
///
/// # Errors
///
/// Returns [`CsvError`] for malformed rows. A first row whose first field is
/// not numeric is treated as a header and skipped.
pub fn parse_points(text: &str) -> Result<Vec<(f64, f64)>, CsvError> {
    let mut points = Vec::new();
    let mut line_number = 0;
    let mut first_row = true;
    for raw in text.lines() {
        line_number += 1;
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        let fields = parse_line(line);
        if fields.len() != 2 {
            if first_row {
                first_row = false;
                continue; // header with a different shape
            }
            return Err(CsvError::WrongColumns {
                line: line_number,
                expected: 2,
                found: fields.len(),
            });
        }
        let x = parse_number(&fields[0], line_number, 1);
        let y = parse_number(&fields[1], line_number, 2);
        if first_row && (x.is_err() || y.is_err()) {
            first_row = false;
            continue; // textual header such as x,y
        }
        first_row = false;
        points.push((x?, y?));
    }
    Ok(points)
}

fn parse_number(field: &str, line: usize, column: usize) -> Result<f64, CsvError> {
    field
        .trim()
        .parse::<f64>()
        .map_err(|_| CsvError::InvalidNumber {
            line,
            column,
            value: field.trim().to_owned(),
        })
}

/// Split one CSV record, honouring double quotes and doubled quotes.
fn parse_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '"' if quoted && chars.peek() == Some(&'"') => {
                current.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => fields.push(std::mem::take(&mut current)),
            _ => current.push(character),
        }
    }
    fields.push(current);
    fields
}

/// Quote a field when it contains a comma, quote or line break.
#[must_use]
pub fn escape(field: &str) -> String {
    if field.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_owned()
    }
}

/// Write a header and rows as CRLF-terminated CSV.
#[must_use]
pub fn write_rows(header: &[&str], rows: &[Vec<String>]) -> String {
    let mut output = String::new();
    output.push_str(
        &header
            .iter()
            .map(|field| escape(field))
            .collect::<Vec<_>>()
            .join(","),
    );
    output.push_str("\r\n");
    for row in rows {
        let fields = row.iter().map(|field| escape(field)).collect::<Vec<_>>();
        output.push_str(&fields.join(","));
        output.push_str("\r\n");
    }
    output
}

fn csv_number(value: f64) -> String {
    if value.is_finite() {
        format_value(value)
    } else {
        String::new()
    }
}

/// Export sampled curves as `x,<name>,...` columns.
#[must_use]
pub fn export_curves(curves: &[(String, &[Sample])]) -> String {
    let mut header = vec!["x".to_owned()];
    header.extend(curves.iter().map(|(name, _)| name.clone()));
    let rows = curves
        .first()
        .map(|(_, points)| *points)
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(index, point)| {
            let mut row = vec![csv_number(point.x)];
            for (_, points) in curves {
                row.push(
                    points
                        .get(index)
                        .map_or_else(String::new, |sample| csv_number(sample.y)),
                );
            }
            row
        })
        .collect::<Vec<_>>();
    write_rows(
        &header.iter().map(String::as_str).collect::<Vec<_>>(),
        &rows,
    )
}

/// Export named variables as `name,value`.
#[must_use]
pub fn export_variables(variables: &[(String, f64)]) -> String {
    let rows = variables
        .iter()
        .map(|(name, value)| vec![name.clone(), csv_number(*value)])
        .collect::<Vec<_>>();
    write_rows(&["name", "value"], &rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_points_with_header_and_crlf() {
        let points = parse_points("x,y\r\n0,1\r\n0.5,2\r\n1,3\r\n").unwrap();
        assert_eq!(points, vec![(0.0, 1.0), (0.5, 2.0), (1.0, 3.0)]);
    }

    #[test]
    fn parses_quoted_fields_and_skips_blank_lines() {
        let points = parse_points("\"x\",\"y\"\n\"1\",\"2\"\n\n3,4\n").unwrap();
        assert_eq!(points, vec![(1.0, 2.0), (3.0, 4.0)]);
    }

    #[test]
    fn reports_line_and_column_for_bad_input() {
        assert_eq!(
            parse_points("x,y\n1,nope"),
            Err(CsvError::InvalidNumber {
                line: 2,
                column: 2,
                value: "nope".to_owned()
            })
        );
        assert_eq!(
            parse_points("x,y\n1"),
            Err(CsvError::WrongColumns {
                line: 2,
                expected: 2,
                found: 1
            })
        );
    }

    #[test]
    fn writes_escaped_rows() {
        let rows = vec![
            vec!["a,b".to_owned(), "say \"hi\"".to_owned()],
            vec!["plain".to_owned(), "2".to_owned()],
        ];
        assert_eq!(
            write_rows(&["name", "value"], &rows),
            "name,value\r\n\"a,b\",\"say \"\"hi\"\"\"\r\nplain,2\r\n"
        );
    }

    #[test]
    fn exports_curves_and_variables() {
        let a = vec![
            Sample { x: 0.0, y: 1.0 },
            Sample {
                x: 1.0,
                y: f64::NAN,
            },
        ];
        let b = vec![Sample { x: 0.0, y: 2.0 }, Sample { x: 1.0, y: 4.0 }];
        let csv = export_curves(&[("A1".to_owned(), &a), ("A2".to_owned(), &b)]);
        assert_eq!(csv, "x,A1,A2\r\n0,1,2\r\n1,,4\r\n");
        assert_eq!(
            export_variables(&[("a".to_owned(), 2.0)]),
            "name,value\r\na,2\r\n"
        );
    }
}
