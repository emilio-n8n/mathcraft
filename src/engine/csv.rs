//! Dependency-free CSV helpers for point series and tabular exports.

use std::fmt;

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
    /// A numeric field could not be parsed or was not finite.
    InvalidNumber {
        /// One-based line number in the source text.
        line: usize,
        /// One-based column number.
        column: usize,
        /// Offending text.
        value: String,
    },
    /// A quoted field is not valid CSV.
    MalformedRecord {
        /// One-based line where the problem was detected.
        line: usize,
        /// Human-readable description.
        message: String,
    },
    /// The input contains no point rows.
    Empty,
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
            Self::MalformedRecord { line, message } => {
                write!(formatter, "line {line}: malformed CSV: {message}")
            }
            Self::Empty => write!(formatter, "CSV contains no data rows"),
        }
    }
}

impl std::error::Error for CsvError {}

#[derive(Debug)]
struct Record {
    fields: Vec<String>,
    line: usize,
}

#[derive(Debug)]
struct Table {
    headers: Vec<String>,
    rows: Vec<Vec<f64>>,
    first_line: usize,
}

/// Parse a two-column `x,y` series, ignoring blank lines and one header row.
///
/// # Errors
///
/// Returns [`CsvError`] for malformed records, non-finite numbers, inconsistent
/// column counts or an input without data rows. A first row is treated as a
/// header only when all of its fields are non-numeric; malformed first data
/// rows are reported instead of being silently discarded.
pub fn parse_points(text: &str) -> Result<Vec<(f64, f64)>, CsvError> {
    let table = parse_table(text)?;
    if table.rows[0].len() != 2 {
        return Err(CsvError::WrongColumns {
            line: table.first_line,
            expected: 2,
            found: table.rows[0].len(),
        });
    }
    Ok(table.rows.into_iter().map(|row| (row[0], row[1])).collect())
}

/// A named series of `(x, y)` points.
pub type PointSeries = (String, Vec<(f64, f64)>);

/// Parse one or more `x,y,z,...` point series from CSV text.
///
/// The first column supplies abscissae and every remaining column becomes a
/// series. Header names are used when present and made unique; otherwise names
/// are generated from `base_name` (`base_name`, `base_name 2`, ...).
///
/// # Errors
///
/// Returns [`CsvError`] for malformed CSV, non-finite values, inconsistent
/// column counts or an input without data rows.
pub fn parse_series(text: &str, base_name: &str) -> Result<Vec<PointSeries>, CsvError> {
    let table = parse_table(text)?;
    let base_name = if base_name.trim().is_empty() {
        "series"
    } else {
        base_name.trim()
    };
    let mut series = Vec::new();
    for (offset, column) in (1..table.rows[0].len()).enumerate() {
        let header = table.headers.get(column).map_or("", String::as_str);
        let name = if header.trim().is_empty() {
            if offset == 0 {
                base_name.to_owned()
            } else {
                format!("{base_name} {}", offset + 1)
            }
        } else {
            header.trim().to_owned()
        };
        let name = unique_name(&name, &series);
        let points = table.rows.iter().map(|row| (row[0], row[column])).collect();
        series.push((name, points));
    }
    Ok(series)
}

fn unique_name(name: &str, series: &[PointSeries]) -> String {
    let mut candidate = name.to_owned();
    let mut suffix = 2;
    while series
        .iter()
        .any(|(existing, _)| existing.eq_ignore_ascii_case(&candidate))
    {
        candidate = format!("{name} {suffix}");
        suffix += 1;
    }
    candidate
}

fn parse_table(text: &str) -> Result<Table, CsvError> {
    let records = parse_records(text)?;
    let Some(first) = records.first() else {
        return Err(CsvError::Empty);
    };
    if first.fields.len() < 2 {
        return Err(CsvError::WrongColumns {
            line: first.line,
            expected: 2,
            found: first.fields.len(),
        });
    }
    let width = first.fields.len();
    let first_is_header = first
        .fields
        .iter()
        .all(|field| field.trim().parse::<f64>().is_err());
    let headers = if first_is_header {
        first.fields.clone()
    } else {
        Vec::new()
    };
    let data = if first_is_header {
        &records[1..]
    } else {
        records.as_slice()
    };
    if data.is_empty() {
        return Err(CsvError::Empty);
    }
    let mut rows = Vec::with_capacity(data.len());
    for record in data {
        if record.fields.len() != width {
            return Err(CsvError::WrongColumns {
                line: record.line,
                expected: width,
                found: record.fields.len(),
            });
        }
        let mut row = Vec::with_capacity(width);
        for (index, field) in record.fields.iter().enumerate() {
            row.push(parse_number(field, record.line, index + 1)?);
        }
        rows.push(row);
    }
    Ok(Table {
        headers,
        rows,
        first_line: first.line,
    })
}

fn parse_number(field: &str, line: usize, column: usize) -> Result<f64, CsvError> {
    let value = field
        .trim()
        .parse::<f64>()
        .map_err(|_| CsvError::InvalidNumber {
            line,
            column,
            value: field.trim().to_owned(),
        })?;
    if !value.is_finite() {
        return Err(CsvError::InvalidNumber {
            line,
            column,
            value: field.trim().to_owned(),
        });
    }
    Ok(value)
}

/// Split CSV records, honouring quoted fields, doubled quotes and embedded
/// newlines. Quotes in unquoted fields and characters after a closing quote are
/// rejected rather than silently repaired.
fn parse_records(text: &str) -> Result<Vec<Record>, CsvError> {
    let mut records = Vec::new();
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut chars = text.chars().peekable();
    let mut line = 1;
    let mut record_line = 1;
    let mut quoted = false;
    let mut after_quote = false;

    while let Some(character) = chars.next() {
        if quoted {
            if character == '"' {
                if chars.peek() == Some(&'"') {
                    current.push('"');
                    chars.next();
                } else {
                    quoted = false;
                    after_quote = true;
                }
            } else if after_quote {
                if matches!(character, ' ' | '\t') {
                    continue;
                }
                return Err(CsvError::MalformedRecord {
                    line,
                    message: "unexpected character after a closing quote".to_owned(),
                });
            } else if character == '\n' {
                current.push('\n');
                line += 1;
            } else if character == '\r' {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                current.push('\n');
                line += 1;
            } else {
                current.push(character);
            }
            continue;
        }

        if after_quote {
            if matches!(character, ' ' | '\t') {
                continue;
            }
            if !matches!(character, ',' | '\n' | '\r') {
                return Err(CsvError::MalformedRecord {
                    line,
                    message: "unexpected character after a closing quote".to_owned(),
                });
            }
        }

        match character {
            '"' if current.is_empty() => {
                quoted = true;
                after_quote = false;
            }
            '"' => {
                return Err(CsvError::MalformedRecord {
                    line: record_line,
                    message: "quote in an unquoted field".to_owned(),
                });
            }
            ',' => {
                fields.push(std::mem::take(&mut current));
                after_quote = false;
            }
            '\n' | '\r' => {
                if character == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                fields.push(std::mem::take(&mut current));
                if !(fields.len() == 1 && fields[0].trim().is_empty()) {
                    records.push(Record {
                        fields: std::mem::take(&mut fields),
                        line: record_line,
                    });
                }
                fields.clear();
                after_quote = false;
                line += 1;
                record_line = line;
            }
            _ => current.push(character),
        }
    }
    if quoted {
        return Err(CsvError::MalformedRecord {
            line: record_line,
            message: "unterminated quoted field".to_owned(),
        });
    }
    fields.push(current);
    if !(fields.len() == 1 && fields[0].trim().is_empty()) {
        records.push(Record {
            fields,
            line: record_line,
        });
    }
    Ok(records)
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
    if !value.is_finite() {
        return String::new();
    }
    if value == 0.0 {
        return "0".to_owned();
    }
    if value.fract() == 0.0 && value.abs() < 1e15 {
        return format!("{value:.0}");
    }
    if value.abs() < 1e-9 || value.abs() >= 1e15 {
        return format!("{value:e}");
    }
    // Rust's shortest round-trip representation avoids losing precision in
    // data exports (display formatting is intentionally less precise).
    value.to_string()
}

/// Export sampled curves as `x,<name>,...` columns.
///
/// The union of all finite abscissae is exported in ascending order. A curve
/// without a sample at a given abscissa contributes an empty field. For
/// duplicate abscissae in one curve, the last sample wins.
#[must_use]
pub fn export_curves(curves: &[(String, &[Sample])]) -> String {
    let mut header = vec!["x".to_owned()];
    header.extend(curves.iter().map(|(name, _)| name.clone()));
    let mut abscissae = curves
        .iter()
        .flat_map(|(_, points)| {
            points
                .iter()
                .filter_map(|point| point.x.is_finite().then_some(point.x))
        })
        .collect::<Vec<_>>();
    abscissae.sort_by(f64::total_cmp);
    abscissae.dedup_by(|left, right| left == right);
    let rows = abscissae
        .into_iter()
        .map(|x| {
            let mut row = vec![csv_number(x)];
            for (_, points) in curves {
                row.push(
                    points
                        .iter()
                        .rev()
                        .find(|sample| sample.x == x)
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
    fn malformed_first_data_is_not_mistaken_for_a_header() {
        assert!(matches!(
            parse_points("1,nope\n2,3"),
            Err(CsvError::InvalidNumber {
                line: 1,
                column: 2,
                ..
            })
        ));
        assert!(matches!(parse_points("x,y\n"), Err(CsvError::Empty)));
        assert!(matches!(parse_points(" \n\t\n"), Err(CsvError::Empty)));
    }

    #[test]
    fn parses_multiple_series_and_uses_unique_header_names() {
        let series = parse_series("x,y,y\n0,1,2\n1,3,4", "data").unwrap();
        assert_eq!(series.len(), 2);
        assert_eq!(series[0].0, "y");
        assert_eq!(series[1].0, "y 2");
        assert_eq!(series[0].1, vec![(0.0, 1.0), (1.0, 3.0)]);
        assert_eq!(series[1].1, vec![(0.0, 2.0), (1.0, 4.0)]);

        let generated = parse_series("0,1,2\n1,3,4", "curve").unwrap();
        assert_eq!(generated[0].0, "curve");
        assert_eq!(generated[1].0, "curve 2");
    }

    #[test]
    fn supports_embedded_newlines_in_quoted_headers() {
        let series = parse_series("x,\"long\nheader\"\n0,1\n1,2", "data").unwrap();
        assert_eq!(series[0].0, "long\nheader");
        assert_eq!(series[0].1.len(), 2);
    }

    #[test]
    fn rejects_malformed_quotes_and_non_finite_values() {
        for text in ["x,y\n\"1,2", "x,y\n1\"x,2", "x,y\n\"1\"x,2"] {
            assert!(
                matches!(parse_points(text), Err(CsvError::MalformedRecord { .. })),
                "{text}"
            );
        }
        for text in ["x,y\nNaN,1", "x,y\ninf,1", "x,y\n1,1e9999"] {
            assert!(matches!(
                parse_points(text),
                Err(CsvError::InvalidNumber { .. })
            ));
        }
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

    #[test]
    fn curve_export_aligns_the_union_of_abscissae_without_losing_precision() {
        let a = vec![Sample {
            x: 1.0,
            y: 1.25e-12,
        }];
        let b = vec![Sample { x: 0.0, y: 2.0 }, Sample { x: 1.0, y: 3.0 }];
        let csv = export_curves(&[("A".to_owned(), &a), ("B".to_owned(), &b)]);
        assert_eq!(csv, "x,A,B\r\n0,,2\r\n1,1.25e-12,3\r\n");
    }
}
