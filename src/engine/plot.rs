//! Pure sampling helpers used by the 2D plot. No drawing code lives here.

use crate::engine::expr::{self, Expr, MathError};

/// Settings for one sampled curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlotOptions {
    /// Left edge of the sampled interval.
    pub x_min: f64,
    /// Right edge of the sampled interval.
    pub x_max: f64,
    /// Number of evenly spaced abscissae, including both endpoints.
    pub samples: usize,
}

impl Default for PlotOptions {
    fn default() -> Self {
        Self {
            x_min: -10.0,
            x_max: 10.0,
            samples: 401,
        }
    }
}

/// One point of a sampled curve. A non-finite `y` marks a gap in the curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    /// Abscissa.
    pub x: f64,
    /// Ordinate, or a non-finite value when evaluation failed.
    pub y: f64,
}

/// Parse `expression` and sample it over the configured interval.
///
/// The variable `x` is bound to each sampled abscissa, overriding any `x`
/// stored in the document. Points where evaluation fails (`1/x` at zero, for
/// example) are kept with a non-finite `y` so the UI can draw a gap.
///
/// # Errors
///
/// Returns a syntax error for malformed expressions and an evaluation error
/// for degenerate sampling settings.
pub fn sample(
    expression: &str,
    variables: &[(String, f64)],
    options: PlotOptions,
) -> Result<Vec<Sample>, MathError> {
    let expr = expr::parse(expression)?;
    sample_expr(&expr, variables, options)
}

/// Sample an already parsed expression.
///
/// # Errors
///
/// Returns an evaluation error when the interval or sample count is invalid.
pub fn sample_expr(
    expr: &Expr,
    variables: &[(String, f64)],
    options: PlotOptions,
) -> Result<Vec<Sample>, MathError> {
    let PlotOptions {
        x_min,
        x_max,
        samples,
    } = options;
    if samples < 2 {
        return Err(MathError::Evaluation {
            message: "a plot needs at least two samples".to_owned(),
        });
    }
    if !x_min.is_finite() || !x_max.is_finite() || x_min >= x_max {
        return Err(MathError::Evaluation {
            message: "the plot interval must be finite and increasing".to_owned(),
        });
    }

    let step = (x_max - x_min) / (samples - 1) as f64;
    let mut result = Vec::with_capacity(samples);
    for index in 0..samples {
        let x = if index + 1 == samples {
            x_max
        } else {
            x_min + step * index as f64
        };
        let mut scoped = variables.to_vec();
        scoped.push(("x".to_owned(), x));
        let y = expr::eval(expr, &scoped).unwrap_or(f64::NAN);
        result.push(Sample { x, y });
    }
    Ok(result)
}

/// Ordinate bounds of the finite samples, widened when the curve is flat.
#[must_use]
pub fn y_bounds(samples: &[Sample]) -> (f64, f64) {
    let mut minimum = f64::INFINITY;
    let mut maximum = f64::NEG_INFINITY;
    for point in samples {
        if point.y.is_finite() {
            minimum = minimum.min(point.y);
            maximum = maximum.max(point.y);
        }
    }
    if !minimum.is_finite() {
        return (-1.0, 1.0);
    }
    if (maximum - minimum).abs() < 1e-12 {
        let padding = if minimum.abs() < 1e-12 {
            1.0
        } else {
            minimum.abs() * 0.1
        };
        (minimum - padding, maximum + padding)
    } else {
        (minimum, maximum)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_a_linear_function() {
        let options = PlotOptions {
            x_min: 0.0,
            x_max: 1.0,
            samples: 3,
        };
        let points = sample("2*x + 1", &[], options).unwrap();
        assert_eq!(points.len(), 3);
        assert_eq!(points[0], Sample { x: 0.0, y: 1.0 });
        assert_eq!(points[1], Sample { x: 0.5, y: 2.0 });
        assert_eq!(points[2], Sample { x: 1.0, y: 3.0 });
    }

    #[test]
    fn samples_a_curve_and_overrides_document_x() {
        let options = PlotOptions {
            x_min: -2.0,
            x_max: 2.0,
            samples: 5,
        };
        let variables = vec![("x".to_owned(), 99.0)];
        let points = sample("x^2", &variables, options).unwrap();
        let ordinates: Vec<f64> = points.iter().map(|point| point.y).collect();
        assert_eq!(ordinates, vec![4.0, 1.0, 0.0, 1.0, 4.0]);
    }

    #[test]
    fn singularities_become_gaps() {
        let options = PlotOptions {
            x_min: -1.0,
            x_max: 1.0,
            samples: 3,
        };
        let points = sample("1/x", &[], options).unwrap();
        assert!(points[0].y.is_finite());
        assert!(points[1].y.is_nan());
        assert!(points[2].y.is_finite());
        let (low, high) = y_bounds(&points);
        assert!((low + 1.0).abs() < 1e-9);
        assert!((high - 1.0).abs() < 1e-9);
    }

    #[test]
    fn flat_curves_get_a_usable_range() {
        let points = vec![Sample { x: 0.0, y: 5.0 }, Sample { x: 1.0, y: 5.0 }];
        let (low, high) = y_bounds(&points);
        assert!(low < 5.0 && high > 5.0);
        assert_eq!(y_bounds(&[]), (-1.0, 1.0));
    }

    #[test]
    fn rejects_bad_settings_and_syntax() {
        let invalid = PlotOptions {
            x_min: 0.0,
            x_max: 1.0,
            samples: 1,
        };
        assert!(sample("x", &[], invalid).is_err());
        assert!(sample("x+", &[], PlotOptions::default()).is_err());
    }
}
