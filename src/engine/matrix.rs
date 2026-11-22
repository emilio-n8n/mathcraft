//! Small dense matrix type with the operations used by MathCraft.

use std::fmt;

/// Errors produced by matrix construction and arithmetic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatrixError {
    /// A matrix cannot have zero rows or zero columns.
    Empty,
    /// Rows do not all have the same length.
    Ragged,
    /// A coefficient entered by the user is not a number.
    InvalidNumber {
        /// The text that could not be parsed.
        token: String,
    },
    /// Operand shapes do not allow the requested operation.
    DimensionMismatch {
        /// Shape of the left operand as `(rows, columns)`.
        left: (usize, usize),
        /// Shape of the right operand as `(rows, columns)`.
        right: (usize, usize),
    },
    /// A square matrix is required but the operand is not square.
    NotSquare {
        /// Actual shape as `(rows, columns)`.
        shape: (usize, usize),
    },
}

impl fmt::Display for MatrixError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(formatter, "a matrix must have at least one row and column"),
            Self::Ragged => write!(formatter, "all matrix rows must have the same length"),
            Self::InvalidNumber { token } => {
                write!(formatter, "invalid matrix coefficient '{token}'")
            }
            Self::DimensionMismatch { left, right } => write!(
                formatter,
                "incompatible matrix sizes {}x{} and {}x{}",
                left.0, left.1, right.0, right.1
            ),
            Self::NotSquare { shape } => write!(
                formatter,
                "a square matrix is required, found {}x{}",
                shape.0, shape.1
            ),
        }
    }
}

impl std::error::Error for MatrixError {}

/// A rectangular, row-major matrix of `f64` values.
#[derive(Debug, Clone, PartialEq)]
pub struct Matrix {
    rows: Vec<Vec<f64>>,
}

impl Matrix {
    /// Build a matrix from rows, rejecting empty or ragged input.
    ///
    /// # Errors
    ///
    /// Returns [`MatrixError::Empty`] or [`MatrixError::Ragged`] for invalid
    /// shapes.
    pub fn new(rows: Vec<Vec<f64>>) -> Result<Self, MatrixError> {
        if rows.is_empty() || rows[0].is_empty() {
            return Err(MatrixError::Empty);
        }
        let width = rows[0].len();
        if rows.iter().any(|row| row.len() != width) {
            return Err(MatrixError::Ragged);
        }
        Ok(Self { rows })
    }

    /// Build a 2x2 matrix from its four coefficients.
    #[must_use]
    pub fn from_2x2(a: f64, b: f64, c: f64, d: f64) -> Self {
        Self {
            rows: vec![vec![a, b], vec![c, d]],
        }
    }

    /// Build an identity matrix of the given order.
    ///
    /// # Errors
    ///
    /// Returns [`MatrixError::Empty`] when `order` is zero.
    pub fn identity(order: usize) -> Result<Self, MatrixError> {
        if order == 0 {
            return Err(MatrixError::Empty);
        }
        let rows = (0..order)
            .map(|row| {
                (0..order)
                    .map(|column| if row == column { 1.0 } else { 0.0 })
                    .collect()
            })
            .collect();
        Ok(Self { rows })
    }

    /// Number of rows.
    #[must_use]
    pub fn rows(&self) -> usize {
        self.rows.len()
    }

    /// Number of columns.
    #[must_use]
    pub fn cols(&self) -> usize {
        self.rows[0].len()
    }

    /// Shape as `(rows, columns)`.
    #[must_use]
    pub fn shape(&self) -> (usize, usize) {
        (self.rows(), self.cols())
    }

    /// Read one coefficient, returning `None` when either index is outside the
    /// matrix.
    #[must_use]
    pub fn get(&self, row: usize, column: usize) -> Option<f64> {
        self.rows
            .get(row)
            .and_then(|cells| cells.get(column))
            .copied()
    }

    /// Borrow the rows in row-major order.
    #[must_use]
    pub fn as_rows(&self) -> &[Vec<f64>] {
        &self.rows
    }

    /// Element-wise addition.
    ///
    /// # Errors
    ///
    /// Returns [`MatrixError::DimensionMismatch`] when the shapes differ.
    pub fn add(&self, other: &Self) -> Result<Self, MatrixError> {
        if self.shape() != other.shape() {
            return Err(MatrixError::DimensionMismatch {
                left: self.shape(),
                right: other.shape(),
            });
        }
        let rows = self
            .rows
            .iter()
            .zip(&other.rows)
            .map(|(left, right)| {
                left.iter()
                    .zip(right)
                    .map(|(a, b)| a + b)
                    .collect::<Vec<_>>()
            })
            .collect();
        Ok(Self { rows })
    }

    /// Element-wise subtraction.
    ///
    /// # Errors
    ///
    /// Returns [`MatrixError::DimensionMismatch`] when the shapes differ.
    pub fn sub(&self, other: &Self) -> Result<Self, MatrixError> {
        if self.shape() != other.shape() {
            return Err(MatrixError::DimensionMismatch {
                left: self.shape(),
                right: other.shape(),
            });
        }
        let rows = self
            .rows
            .iter()
            .zip(&other.rows)
            .map(|(left, right)| {
                left.iter()
                    .zip(right)
                    .map(|(a, b)| a - b)
                    .collect::<Vec<_>>()
            })
            .collect();
        Ok(Self { rows })
    }

    /// Standard matrix product `self * other`.
    ///
    /// # Errors
    ///
    /// Returns [`MatrixError::DimensionMismatch`] when the inner dimensions do
    /// not agree.
    pub fn mul(&self, other: &Self) -> Result<Self, MatrixError> {
        if self.cols() != other.rows() {
            return Err(MatrixError::DimensionMismatch {
                left: self.shape(),
                right: other.shape(),
            });
        }
        let rows = (0..self.rows())
            .map(|row| {
                (0..other.cols())
                    .map(|column| {
                        (0..self.cols())
                            .map(|k| self.rows[row][k] * other.rows[k][column])
                            .sum()
                    })
                    .collect()
            })
            .collect();
        Ok(Self { rows })
    }

    /// Multiply every coefficient by a scalar.
    #[must_use]
    pub fn scale(&self, factor: f64) -> Self {
        Self {
            rows: self
                .rows
                .iter()
                .map(|row| row.iter().map(|value| value * factor).collect())
                .collect(),
        }
    }

    /// Transpose across the main diagonal.
    #[must_use]
    pub fn transpose(&self) -> Self {
        let rows = (0..self.cols())
            .map(|column| (0..self.rows()).map(|row| self.rows[row][column]).collect())
            .collect();
        Self { rows }
    }

    /// Determinant computed by Gaussian elimination with partial pivoting.
    ///
    /// # Errors
    ///
    /// Returns [`MatrixError::NotSquare`] for non-square matrices.
    pub fn det(&self) -> Result<f64, MatrixError> {
        if self.rows() != self.cols() {
            return Err(MatrixError::NotSquare {
                shape: self.shape(),
            });
        }
        let order = self.rows();
        let mut values = self.rows.clone();
        let mut determinant = 1.0;

        for column in 0..order {
            let mut pivot = column;
            for row in column + 1..order {
                if values[row][column].abs() > values[pivot][column].abs() {
                    pivot = row;
                }
            }
            if values[pivot][column].abs() < 1e-12 {
                return Ok(0.0);
            }
            if pivot != column {
                values.swap(column, pivot);
                determinant = -determinant;
            }
            let diagonal = values[column][column];
            determinant *= diagonal;
            for row in column + 1..order {
                let factor = values[row][column] / diagonal;
                for cell in column..order {
                    values[row][cell] -= factor * values[column][cell];
                }
            }
        }
        Ok(determinant)
    }
}

impl fmt::Display for Matrix {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, row) in self.rows.iter().enumerate() {
            if index > 0 {
                writeln!(formatter)?;
            }
            write!(formatter, "[")?;
            for (column, value) in row.iter().enumerate() {
                if column > 0 {
                    write!(formatter, " ")?;
                }
                write!(formatter, "{}", crate::engine::format_value(*value))?;
            }
            write!(formatter, "]")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_rectangular_matrices() {
        let matrix = Matrix::new(vec![vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]).unwrap();
        assert_eq!(matrix.shape(), (2, 3));
        assert_eq!(matrix.get(1, 2), Some(6.0));
        assert_eq!(matrix.get(2, 0), None);
        assert_eq!(matrix.as_rows().len(), 2);
    }

    #[test]
    fn rejects_empty_and_ragged_input() {
        assert_eq!(Matrix::new(vec![]), Err(MatrixError::Empty));
        assert_eq!(Matrix::new(vec![vec![]]), Err(MatrixError::Empty));
        assert_eq!(
            Matrix::new(vec![vec![1.0], vec![1.0, 2.0]]),
            Err(MatrixError::Ragged)
        );
        assert_eq!(Matrix::identity(0), Err(MatrixError::Empty));
    }

    #[test]
    fn addition_and_subtraction_are_element_wise() {
        let a = Matrix::from_2x2(1.0, 2.0, 3.0, 4.0);
        let b = Matrix::from_2x2(10.0, 20.0, 30.0, 40.0);
        assert_eq!(a.add(&b).unwrap(), Matrix::from_2x2(11.0, 22.0, 33.0, 44.0));
        assert_eq!(b.sub(&a).unwrap(), Matrix::from_2x2(9.0, 18.0, 27.0, 36.0));
    }

    #[test]
    fn multiplication_follows_linear_algebra_rules() {
        let a = Matrix::new(vec![vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]).unwrap();
        let b = Matrix::new(vec![vec![7.0, 8.0], vec![9.0, 10.0], vec![11.0, 12.0]]).unwrap();
        let product = a.mul(&b).unwrap();
        assert_eq!(
            product,
            Matrix::new(vec![vec![58.0, 64.0], vec![139.0, 154.0]]).unwrap()
        );
        assert_eq!(
            a.mul(&a),
            Err(MatrixError::DimensionMismatch {
                left: (2, 3),
                right: (2, 3),
            })
        );
    }

    #[test]
    fn identity_is_neutral_for_multiplication() {
        let a = Matrix::from_2x2(1.0, 2.0, 3.0, 4.0);
        let identity = Matrix::identity(2).unwrap();
        assert_eq!(a.mul(&identity).unwrap(), a);
        assert_eq!(identity.mul(&a).unwrap(), a);
    }

    #[test]
    fn transpose_swaps_rows_and_columns() {
        let a = Matrix::new(vec![vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]).unwrap();
        assert_eq!(
            a.transpose(),
            Matrix::new(vec![vec![1.0, 4.0], vec![2.0, 5.0], vec![3.0, 6.0]]).unwrap()
        );
        assert_eq!(a.transpose().transpose(), a);
    }

    #[test]
    fn determinant_handles_two_by_two_and_three_by_three() {
        assert_eq!(Matrix::from_2x2(1.0, 2.0, 3.0, 4.0).det().unwrap(), -2.0);
        let singular = Matrix::from_2x2(1.0, 2.0, 2.0, 4.0);
        assert_eq!(singular.det().unwrap(), 0.0);
        let three = Matrix::new(vec![
            vec![6.0, 1.0, 1.0],
            vec![4.0, -2.0, 5.0],
            vec![2.0, 8.0, 7.0],
        ])
        .unwrap();
        assert_eq!(three.det().unwrap(), -306.0);
    }

    #[test]
    fn determinant_requires_a_square_matrix() {
        let matrix = Matrix::new(vec![vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]).unwrap();
        assert_eq!(matrix.det(), Err(MatrixError::NotSquare { shape: (2, 3) }));
    }

    #[test]
    fn scaling_and_display() {
        let matrix = Matrix::from_2x2(1.0, -2.0, 3.5, 4.0);
        assert_eq!(matrix.scale(2.0), Matrix::from_2x2(2.0, -4.0, 7.0, 8.0));
        assert_eq!(matrix.to_string(), "[1 -2]\n[3.5 4]");
    }
}
