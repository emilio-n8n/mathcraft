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
    if samples > 100_000 {
        return Err(MathError::Evaluation {
            message: "a plot accepts at most 100000 samples".to_owned(),
        });
    }
    if !x_min.is_finite() || !x_max.is_finite() || x_min >= x_max {
        return Err(MathError::Evaluation {
            message: "the plot interval must be finite and increasing".to_owned(),
        });
    }

    let span = x_max - x_min;
    let mut result = Vec::with_capacity(samples);
    for index in 0..samples {
        let x = if index + 1 == samples {
            x_max
        } else if span.is_finite() {
            let fraction = index as f64 / (samples - 1) as f64;
            x_min + span * fraction
        } else {
            let fraction = index as f64 / (samples - 1) as f64;
            x_min * (1.0 - fraction) + x_max * fraction
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
    if maximum == minimum {
        let padding = if minimum.abs() < 1e-12 {
            1.0
        } else {
            minimum.abs() * 0.1
        };
        let low = minimum - padding;
        let high = maximum + padding;
        if low.is_finite() && high.is_finite() && low < high {
            return (low, high);
        }
        // Avoid overflow when a flat curve is close to f64::MAX. There is no
        // finite value above f64::MAX, so keep the curve on the upper edge.
        let padding = minimum.abs() * 0.5;
        if minimum > 0.0 {
            return (minimum - padding, minimum);
        }
        return (minimum, minimum + padding);
    }
    (minimum, maximum)
}

/// Map a value to a zero-to-one coordinate without overflowing on wide ranges.
///
/// Invalid or collapsed bounds fall back to the midpoint. Finite inputs outside
/// the bounds are deliberately not clamped so callers can clip the resulting
/// line geometry.
#[must_use]
pub fn normalized(value: f64, minimum: f64, maximum: f64) -> f64 {
    if !value.is_finite() || !minimum.is_finite() || !maximum.is_finite() || minimum >= maximum {
        return 0.5;
    }
    let span = maximum - minimum;
    let numerator = value - minimum;
    if span.is_finite() && numerator.is_finite() {
        numerator / span
    } else {
        (value * 0.5 - minimum * 0.5) / (maximum * 0.5 - minimum * 0.5)
    }
}

/// Generate human-friendly tick values inside an inclusive axis range.
///
/// At most 100 ticks are returned. Degenerate or non-finite ranges have no
/// ticks, allowing callers to keep the last valid view instead of drawing
/// misleading coordinates.
#[must_use]
pub fn axis_ticks(minimum: f64, maximum: f64, target_count: usize) -> Vec<f64> {
    if !minimum.is_finite() || !maximum.is_finite() || minimum >= maximum {
        return Vec::new();
    }
    let target_count = target_count.clamp(2, 100);
    let raw_step = (maximum - minimum) / (target_count - 1) as f64;
    if !raw_step.is_finite() || raw_step <= 0.0 {
        return vec![minimum, maximum];
    }
    let magnitude = 10_f64.powf(raw_step.log10().floor());
    if !magnitude.is_finite() || magnitude <= 0.0 {
        return vec![minimum, maximum];
    }
    let fraction = raw_step / magnitude;
    let nice = if fraction <= 1.0 {
        1.0
    } else if fraction <= 2.0 {
        2.0
    } else if fraction <= 5.0 {
        5.0
    } else {
        10.0
    };
    let step = nice * magnitude;
    if !step.is_finite() || step <= 0.0 {
        return vec![minimum, maximum];
    }
    let first = (minimum / step).ceil() * step;
    let mut ticks = Vec::new();
    let mut value = first;
    while value <= maximum && ticks.len() < 100 {
        ticks.push(value);
        value += step;
    }
    if ticks.is_empty() {
        vec![minimum, maximum]
    } else {
        ticks
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
        let excessive = PlotOptions {
            samples: 100_001,
            ..PlotOptions::default()
        };
        assert!(sample("x", &[], excessive).is_err());
    }

    #[test]
    fn sampling_wide_finite_intervals_does_not_overflow() {
        let options = PlotOptions {
            x_min: -1e308,
            x_max: 1e308,
            samples: 3,
        };
        let points = sample("x", &[], options).unwrap();
        assert_eq!(points[0].x, -1e308);
        assert_eq!(points[1].x, 0.0);
        assert_eq!(points[2].x, 1e308);
        assert!((normalized(0.0, -1e308, 1e308) - 0.5).abs() < 1e-12);
    }

    #[test]
    fn flat_extreme_curves_get_finite_padded_bounds() {
        let (low, high) = y_bounds(&[Sample {
            x: 0.0,
            y: f64::MAX,
        }]);
        assert!(low.is_finite() && high.is_finite() && low < f64::MAX);
        assert!(high > f64::MAX * 0.9);
    }

    #[test]
    fn axis_ticks_use_readable_steps_and_reject_bad_ranges() {
        assert_eq!(axis_ticks(0.0, 1.0, 5), vec![0.0, 0.5, 1.0]);
        assert_eq!(axis_ticks(-1.0, 1.0, 5), vec![-1.0, -0.5, 0.0, 0.5, 1.0]);
        assert!(axis_ticks(1.0, 0.0, 5).is_empty());
        assert_eq!(axis_ticks(-1e308, 1e308, 5), vec![-1e308, 1e308]);
    }
}
