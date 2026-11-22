//! Tokenizer, recursive-descent parser and evaluator for scalar expressions.
//!
//! The grammar, from lowest to highest precedence:
//!
//! ```text
//! expr    := term (('+' | '-') term)*
//! term    := unary (('*' | '/') unary)*
//! unary   := ('+' | '-') unary | power
//! power   := primary ('^' unary)?
//! primary := number | ident [ '(' args ')' ] | '(' expr ')'
//! ```
//!
//! Power is right-associative and binds tighter than unary minus, so `-2^2`
//! evaluates to `-4` while `2^-2` evaluates to `0.25`.

use std::fmt;

/// A lexical token together with its byte offset in the source text.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    /// Token payload.
    pub kind: TokenKind,
    /// Byte offset of the token's first character.
    pub pos: usize,
}

/// Payloads recognised by the expression tokenizer.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    /// Numeric literal, for example `2` or `3.5`.
    Number(f64),
    /// Variable, function or constant name.
    Ident(String),
    /// `+`
    Plus,
    /// `-`
    Minus,
    /// `*`
    Star,
    /// `/`
    Slash,
    /// `^`
    Caret,
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `,`
    Comma,
}

impl TokenKind {
    fn description(&self) -> String {
        match self {
            Self::Number(value) => format!("number {value}"),
            Self::Ident(name) => format!("identifier '{name}'"),
            Self::Plus => "'+'".to_owned(),
            Self::Minus => "'-'".to_owned(),
            Self::Star => "'*'".to_owned(),
            Self::Slash => "'/'".to_owned(),
            Self::Caret => "'^'".to_owned(),
            Self::LParen => "'('".to_owned(),
            Self::RParen => "')'".to_owned(),
            Self::Comma => "','".to_owned(),
        }
    }
}

/// An error with enough context to show a useful message in the UI.
#[derive(Debug, Clone, PartialEq)]
pub enum MathError {
    /// Malformed input, with an optional byte offset into the source.
    Syntax {
        /// Human-readable description of the problem.
        message: String,
        /// Byte offset where the problem was detected.
        pos: Option<usize>,
    },
    /// A variable name is not present in the document.
    UnknownVariable {
        /// Name that could not be resolved.
        name: String,
        /// Byte offset of the variable reference.
        pos: usize,
    },
    /// A function name is not part of the supported set.
    UnknownFunction {
        /// Name that could not be resolved.
        name: String,
        /// Byte offset of the call.
        pos: usize,
    },
    /// The expression parsed but could not be evaluated.
    Evaluation {
        /// Human-readable description of the domain or arity problem.
        message: String,
    },
}

impl MathError {
    fn syntax(message: impl Into<String>, pos: Option<usize>) -> Self {
        Self::Syntax {
            message: message.into(),
            pos,
        }
    }
}

impl fmt::Display for MathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax { message, pos } => match pos {
                Some(pos) => write!(formatter, "{message} (position {pos})"),
                None => write!(formatter, "{message}"),
            },
            Self::UnknownVariable { name, pos } => {
                write!(formatter, "unknown variable '{name}' (position {pos})")
            }
            Self::UnknownFunction { name, pos } => {
                write!(formatter, "unknown function '{name}' (position {pos})")
            }
            Self::Evaluation { message } => write!(formatter, "{message}"),
        }
    }
}

impl std::error::Error for MathError {}

/// A parsed expression node, carrying the source position of its first token.
#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    /// Node payload.
    pub kind: ExprKind,
    /// Byte offset of the node in the source expression.
    pub pos: usize,
}

impl Expr {
    fn new(kind: ExprKind, pos: usize) -> Self {
        Self { kind, pos }
    }
}

/// Payloads of a parsed expression tree.
#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    /// Numeric literal or named constant.
    Number(f64),
    /// Reference to a document variable.
    Variable(String),
    /// Unary sign operation.
    Unary {
        /// Operator to apply.
        op: UnaryOp,
        /// Operand.
        value: Box<Expr>,
    },
    /// Binary arithmetic operation.
    Binary {
        /// Operator to apply.
        op: BinaryOp,
        /// Left operand.
        left: Box<Expr>,
        /// Right operand.
        right: Box<Expr>,
    },
    /// Call to a supported function.
    Call {
        /// Function name as written by the user.
        name: String,
        /// Argument expressions.
        args: Vec<Expr>,
    },
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    /// Unary plus.
    Plus,
    /// Unary minus.
    Minus,
}

/// Binary arithmetic operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    /// Addition.
    Add,
    /// Subtraction.
    Sub,
    /// Multiplication.
    Mul,
    /// Division.
    Div,
    /// Exponentiation.
    Pow,
}

/// Split a source string into positioned tokens.
///
/// # Errors
///
/// Returns [`MathError::Syntax`] for an unrecognised character or a malformed
/// numeric literal.
pub fn tokenize(input: &str) -> Result<Vec<Token>, MathError> {
    let bytes = input.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;

    while index < bytes.len() {
        let byte = bytes[index];
        if byte.is_ascii_whitespace() {
            index += 1;
            continue;
        }
        let pos = index;
        let kind = match byte {
            b'0'..=b'9' | b'.' => {
                let mut end = index;
                let mut dots = 0;
                while end < bytes.len() && (bytes[end].is_ascii_digit() || bytes[end] == b'.') {
                    if bytes[end] == b'.' {
                        dots += 1;
                        if dots > 1 {
                            return Err(MathError::syntax("invalid number", Some(end)));
                        }
                    }
                    end += 1;
                }
                let text = &input[index..end];
                let value = text
                    .parse::<f64>()
                    .map_err(|_| MathError::syntax("invalid number", Some(pos)))?;
                index = end;
                TokenKind::Number(value)
            }
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                let mut end = index + 1;
                while end < bytes.len()
                    && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_')
                {
                    end += 1;
                }
                let name = input[index..end].to_owned();
                index = end;
                TokenKind::Ident(name)
            }
            b'+' => {
                index += 1;
                TokenKind::Plus
            }
            b'-' => {
                index += 1;
                TokenKind::Minus
            }
            b'*' => {
                index += 1;
                TokenKind::Star
            }
            b'/' => {
                index += 1;
                TokenKind::Slash
            }
            b'^' => {
                index += 1;
                TokenKind::Caret
            }
            b'(' => {
                index += 1;
                TokenKind::LParen
            }
            b')' => {
                index += 1;
                TokenKind::RParen
            }
            b',' => {
                index += 1;
                TokenKind::Comma
            }
            _ => {
                let character = input[index..].chars().next().unwrap_or('\u{fffd}');
                return Err(MathError::syntax(
                    format!("unexpected character '{character}'"),
                    Some(pos),
                ));
            }
        };
        tokens.push(Token { kind, pos });
    }
    Ok(tokens)
}

/// Parse a source expression into an expression tree.
///
/// # Errors
///
/// Returns an actionable [`MathError`] containing the offending token and its
/// byte position when the input is malformed.
pub fn parse(input: &str) -> Result<Expr, MathError> {
    let tokens = tokenize(input)?;
    let mut parser = Parser {
        tokens: &tokens,
        index: 0,
        source_len: input.len(),
    };
    let expr = parser.parse_expr()?;
    if let Some(token) = parser.peek() {
        return Err(MathError::syntax(
            format!("unexpected {}", token.kind.description()),
            Some(token.pos),
        ));
    }
    Ok(expr)
}

struct Parser<'a> {
    tokens: &'a [Token],
    index: usize,
    source_len: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&'a Token> {
        self.tokens.get(self.index)
    }

    fn at(&self, kind: &TokenKind) -> bool {
        self.peek().is_some_and(|token| token.kind == *kind)
    }

    fn advance(&mut self) {
        self.index += 1;
    }

    fn expect(&mut self, kind: TokenKind, label: &str) -> Result<&'a Token, MathError> {
        if self.at(&kind) {
            let token = &self.tokens[self.index];
            self.advance();
            Ok(token)
        } else if let Some(token) = self.peek() {
            Err(MathError::syntax(
                format!("expected {label}, found {}", token.kind.description()),
                Some(token.pos),
            ))
        } else {
            Err(MathError::syntax(
                format!("expected {label}, found end of expression"),
                Some(self.source_len),
            ))
        }
    }

    fn parse_expr(&mut self) -> Result<Expr, MathError> {
        let mut left = self.parse_term()?;
        loop {
            let op = match self.peek().map(|token| &token.kind) {
                Some(TokenKind::Plus) => BinaryOp::Add,
                Some(TokenKind::Minus) => BinaryOp::Sub,
                _ => break,
            };
            let pos = left.pos;
            self.advance();
            let right = self.parse_term()?;
            left = Expr::new(
                ExprKind::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                pos,
            );
        }
        Ok(left)
    }

    fn parse_term(&mut self) -> Result<Expr, MathError> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.peek().map(|token| &token.kind) {
                Some(TokenKind::Star) => BinaryOp::Mul,
                Some(TokenKind::Slash) => BinaryOp::Div,
                _ => break,
            };
            let pos = left.pos;
            self.advance();
            let right = self.parse_unary()?;
            left = Expr::new(
                ExprKind::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                pos,
            );
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, MathError> {
        let op = match self.peek().map(|token| &token.kind) {
            Some(TokenKind::Plus) => Some(UnaryOp::Plus),
            Some(TokenKind::Minus) => Some(UnaryOp::Minus),
            _ => None,
        };
        if let Some(op) = op {
            let pos = self.peek().map_or(self.source_len, |token| token.pos);
            self.advance();
            let value = self.parse_unary()?;
            return Ok(Expr::new(
                ExprKind::Unary {
                    op,
                    value: Box::new(value),
                },
                pos,
            ));
        }
        self.parse_power()
    }

    fn parse_power(&mut self) -> Result<Expr, MathError> {
        let base = self.parse_primary()?;
        if !self.at(&TokenKind::Caret) {
            return Ok(base);
        }
        let pos = base.pos;
        self.advance();
        let exponent = self.parse_unary()?;
        Ok(Expr::new(
            ExprKind::Binary {
                op: BinaryOp::Pow,
                left: Box::new(base),
                right: Box::new(exponent),
            },
            pos,
        ))
    }

    fn parse_primary(&mut self) -> Result<Expr, MathError> {
        match self.peek() {
            Some(Token {
                kind: TokenKind::Number(value),
                pos,
            }) => {
                let (value, pos) = (*value, *pos);
                self.advance();
                Ok(Expr::new(ExprKind::Number(value), pos))
            }
            Some(Token {
                kind: TokenKind::Ident(name),
                pos,
            }) => {
                let (name, pos) = (name.clone(), *pos);
                self.advance();
                if self.at(&TokenKind::LParen) {
                    self.advance();
                    let mut args = Vec::new();
                    if self.at(&TokenKind::RParen) {
                        let close = self.peek().map_or(self.source_len, |token| token.pos);
                        return Err(MathError::syntax(
                            "a function call needs at least one argument",
                            Some(close),
                        ));
                    }
                    loop {
                        args.push(self.parse_expr()?);
                        if self.at(&TokenKind::Comma) {
                            self.advance();
                            continue;
                        }
                        break;
                    }
                    self.expect(TokenKind::RParen, "')'")?;
                    Ok(Expr::new(ExprKind::Call { name, args }, pos))
                } else {
                    match name.to_ascii_lowercase().as_str() {
                        "pi" => Ok(Expr::new(ExprKind::Number(std::f64::consts::PI), pos)),
                        "e" => Ok(Expr::new(ExprKind::Number(std::f64::consts::E), pos)),
                        _ => Ok(Expr::new(ExprKind::Variable(name), pos)),
                    }
                }
            }
            Some(Token {
                kind: TokenKind::LParen,
                ..
            }) => {
                self.advance();
                let inner = self.parse_expr()?;
                self.expect(TokenKind::RParen, "')'")?;
                Ok(inner)
            }
            Some(token) => Err(MathError::syntax(
                format!("unexpected {}", token.kind.description()),
                Some(token.pos),
            )),
            None => Err(MathError::syntax(
                "unexpected end of expression",
                Some(self.source_len),
            )),
        }
    }
}

/// Evaluate a parsed expression against a set of named variables.
///
/// Later entries in `variables` take precedence, which lets plot sampling
/// override a document variable named `x` with the current abscissa.
///
/// # Errors
///
/// Returns an error for unknown names, invalid arity, division by zero and
/// out-of-domain function arguments.
pub fn eval(expr: &Expr, variables: &[(String, f64)]) -> Result<f64, MathError> {
    match &expr.kind {
        ExprKind::Number(value) => Ok(*value),
        ExprKind::Variable(name) => variables
            .iter()
            .rev()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, value)| *value)
            .ok_or_else(|| MathError::UnknownVariable {
                name: name.clone(),
                pos: expr.pos,
            }),
        ExprKind::Unary { op, value } => {
            let value = eval(value, variables)?;
            match op {
                UnaryOp::Plus => Ok(value),
                UnaryOp::Minus => Ok(-value),
            }
        }
        ExprKind::Binary { op, left, right } => {
            let left = eval(left, variables)?;
            let right = eval(right, variables)?;
            match op {
                BinaryOp::Add => Ok(left + right),
                BinaryOp::Sub => Ok(left - right),
                BinaryOp::Mul => Ok(left * right),
                BinaryOp::Div => {
                    if right == 0.0 {
                        Err(MathError::Evaluation {
                            message: "division by zero".to_owned(),
                        })
                    } else {
                        Ok(left / right)
                    }
                }
                BinaryOp::Pow => {
                    let result = left.powf(right);
                    if result.is_finite() {
                        Ok(result)
                    } else {
                        Err(MathError::Evaluation {
                            message: "invalid power".to_owned(),
                        })
                    }
                }
            }
        }
        ExprKind::Call { name, args } => {
            let values = args
                .iter()
                .map(|arg| eval(arg, variables))
                .collect::<Result<Vec<_>, _>>()?;
            call(name, &values, expr.pos)
        }
    }
}

fn call(name: &str, args: &[f64], pos: usize) -> Result<f64, MathError> {
    let arity = |expected: usize| {
        if args.len() == expected {
            Ok(())
        } else {
            Err(MathError::syntax(
                format!("{name} expects {expected} argument(s), got {}", args.len()),
                Some(pos),
            ))
        }
    };
    let one = || {
        arity(1)?;
        Ok(args[0])
    };
    let positive = |value: f64, function: &str| {
        if value > 0.0 {
            Ok(value)
        } else {
            Err(MathError::Evaluation {
                message: format!("{function} requires a positive argument"),
            })
        }
    };

    match name.to_ascii_lowercase().as_str() {
        "sin" => Ok(one()?.sin()),
        "cos" => Ok(one()?.cos()),
        "tan" => Ok(one()?.tan()),
        "sqrt" => {
            let value = one()?;
            if value < 0.0 {
                Err(MathError::Evaluation {
                    message: "sqrt requires a non-negative argument".to_owned(),
                })
            } else {
                Ok(value.sqrt())
            }
        }
        "abs" => Ok(one()?.abs()),
        "ln" => Ok(positive(one()?, "ln")?.ln()),
        "exp" => Ok(one()?.exp()),
        "log" => match args.len() {
            1 => Ok(positive(args[0], "log")?.log10()),
            2 => {
                let value = positive(args[0], "log")?;
                let base = positive(args[1], "log base")?;
                if (base - 1.0).abs() < f64::EPSILON {
                    Err(MathError::Evaluation {
                        message: "log base must not be 1".to_owned(),
                    })
                } else {
                    Ok(value.log(base))
                }
            }
            other => Err(MathError::syntax(
                format!("log expects 1 or 2 arguments, got {other}"),
                Some(pos),
            )),
        },
        _ => Err(MathError::UnknownFunction {
            name: name.to_owned(),
            pos,
        }),
    }
}

/// Parse and evaluate an expression in one step.
///
/// # Errors
///
/// Propagates both syntax and evaluation errors from [`parse`] and [`eval`].
pub fn evaluate(expression: &str, variables: &[(String, f64)]) -> Result<f64, MathError> {
    let expr = parse(expression)?;
    eval(&expr, variables)
}

/// Format a computed value for display without long floating-point tails.
#[must_use]
pub fn format_value(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value == f64::INFINITY {
        return "infinity".to_owned();
    }
    if value == f64::NEG_INFINITY {
        return "-infinity".to_owned();
    }
    if value == 0.0 {
        return "0".to_owned();
    }
    if value.fract() == 0.0 && value.abs() < 1e15 {
        return format!("{value:.0}");
    }
    let text = format!("{value:.10}");
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() {
        "0".to_owned()
    } else {
        trimmed.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn number(expression: &str) -> f64 {
        evaluate(expression, &[]).unwrap_or_else(|error| panic!("{expression}: {error}"))
    }

    fn close(expression: &str, expected: f64) {
        let actual = number(expression);
        assert!(
            (actual - expected).abs() < 1e-9,
            "{expression}: expected {expected}, got {actual}"
        );
    }

    #[test]
    fn precedence_multiplication_before_addition() {
        close("2+3*4", 14.0);
        close("10-6/2", 7.0);
    }

    #[test]
    fn parentheses_override_precedence() {
        close("(2+3)*4", 20.0);
        close("2*(3+4)-1", 13.0);
    }

    #[test]
    fn power_is_right_associative_and_binds_before_sign() {
        close("2^3^2", 512.0);
        close("-2^2", -4.0);
        close("(-2)^2", 4.0);
        close("2^-2", 0.25);
    }

    #[test]
    fn unary_signs_and_constants() {
        close("--3", 3.0);
        close("pi", std::f64::consts::PI);
        close("e", std::f64::consts::E);
    }

    #[test]
    fn functions_and_optional_log_base() {
        close("sin(0)", 0.0);
        close("cos(0)", 1.0);
        close("tan(0)", 0.0);
        close("sqrt(16)", 4.0);
        close("abs(-3)", 3.0);
        close("ln(e)", 1.0);
        close("log(100)", 2.0);
        close("log(8,2)", 3.0);
        close("exp(0)", 1.0);
    }

    #[test]
    fn variables_are_resolved_from_the_document() {
        let variables = vec![("a".to_owned(), 2.0), ("b".to_owned(), 3.0)];
        let value = evaluate("a*b+1", &variables).unwrap();
        assert!((value - 7.0).abs() < 1e-9);
    }

    #[test]
    fn unknown_variable_reports_name_and_position() {
        let error = evaluate("1 + alpha", &[]).unwrap_err();
        assert_eq!(
            error,
            MathError::UnknownVariable {
                name: "alpha".to_owned(),
                pos: 4
            }
        );
        assert!(error.to_string().contains("position 4"));
    }

    #[test]
    fn invalid_syntax_is_actionable() {
        for input in ["2+", "(2+3", "2 3", "sqrt()", "2..3", "1 $ 2"] {
            let error = evaluate(input, &[]).unwrap_err();
            assert!(
                matches!(error, MathError::Syntax { .. }),
                "{input} produced {error:?}"
            );
            assert!(
                error.to_string().contains("position"),
                "{input} produced {error}"
            );
        }
    }

    #[test]
    fn unknown_functions_and_domain_errors() {
        assert!(matches!(
            evaluate("foo(1)", &[]),
            Err(MathError::UnknownFunction { .. })
        ));
        assert!(evaluate("1/0", &[]).is_err());
        assert!(evaluate("sqrt(-1)", &[]).is_err());
        assert!(evaluate("ln(0)", &[]).is_err());
        assert!(evaluate("sin(1,2)", &[]).is_err());
    }

    #[test]
    fn token_positions_are_recorded() {
        let tokens = tokenize("x + 12").unwrap();
        assert_eq!(tokens[0].pos, 0);
        assert_eq!(tokens[2].pos, 4);
    }

    #[test]
    fn value_formatting_is_short() {
        assert_eq!(format_value(2.0), "2");
        assert_eq!(format_value(0.1 + 0.2), "0.3");
        assert_eq!(format_value(0.0), "0");
    }
}
