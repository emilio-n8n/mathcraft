//! Tokenizer, recursive-descent parser and evaluator for scalar expressions.
//!
//! The grammar, from lowest to highest precedence:
//!
//! ```text
//! expr    := term (('+' | '-') term)*
//! term    := unary (('*' | '/' | implicit) unary)*
//! unary   := ('+' | '-') unary | power
//! power   := primary ('^' unary)?
//! primary := number | ident [ '(' args ')' ] | '(' expr ')'
//! ```
//!
//! Power is right-associative and binds tighter than unary minus, so `-2^2`
//! evaluates to `-4` while `2^-2` evaluates to `0.25`. Adjacent operands imply
//! multiplication (`2x`, `2(x+1)`, `pi(x+1)`), except when the identifier is a
//! known function name followed by `(`. Scientific notation (`1.5e-3`) is
//! accepted in numeric literals; a bare `e` remains Euler's constant.
//!
//! Worksheet references may be qualified (`Plot.A1`). Sheet names containing
//! spaces or punctuation can be single- or double-quoted (`'My Sheet'.A1`).
//! Token and nesting limits keep malformed external input from exhausting the
//! process stack.

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
    /// Variable, function, constant or qualified worksheet reference.
    Ident(String),
    /// A quoted sheet or variable name containing punctuation.
    QuotedIdentifier(String),
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

/// Maximum number of tokens accepted in one expression.
pub const MAX_TOKENS: usize = 1024;
/// Maximum recursive nesting accepted by the parser.
pub const MAX_PARSE_DEPTH: usize = 256;
/// Maximum evaluation depth, covering every token in a bounded expression.
pub const MAX_EVAL_DEPTH: usize = MAX_TOKENS + 1;

/// Function names understood by the evaluator, also used for syntax help.
pub const FUNCTION_NAMES: &[&str] = &[
    "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "sqrt", "abs", "ln", "log", "exp", "pow",
    "round", "floor", "ceil", "trunc", "sign", "min", "max", "sum", "mean",
];

/// True when an identifier followed by `(` is a function call rather than an
/// implicit multiplication.
#[must_use]
pub fn is_function_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    FUNCTION_NAMES.contains(&lower.as_str())
}

/// Whether `name` is one unquoted identifier accepted by the tokenizer.
///
/// This is useful when validating names entered through the UI; quoted names
/// remain available for worksheet references in expressions.
#[must_use]
pub fn is_identifier(name: &str) -> bool {
    matches!(tokenize(name), Ok(tokens) if tokens.len() == 1 && matches!(&tokens[0].kind, TokenKind::Ident(_)))
}

impl TokenKind {
    fn description(&self) -> String {
        match self {
            Self::Number(value) => format!("number {value}"),
            Self::Ident(name) | Self::QuotedIdentifier(name) => {
                format!("identifier '{name}'")
            }
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

    /// Byte offset associated with this error, when available.
    #[must_use]
    pub fn position(&self) -> Option<usize> {
        match self {
            Self::Syntax { pos, .. } => *pos,
            Self::UnknownVariable { pos, .. } | Self::UnknownFunction { pos, .. } => Some(*pos),
            Self::Evaluation { .. } => None,
        }
    }

    /// Format an error with a one-based line, column and source excerpt.
    ///
    /// The caret uses Unicode character columns while the regular [`Display`]
    /// implementation keeps reporting byte offsets for machine-readable use.
    #[must_use]
    pub fn diagnostic(&self, source: &str) -> String {
        let message = match self {
            Self::Syntax { message, .. } => message.clone(),
            Self::UnknownVariable { name, .. } => format!("unknown variable '{name}'"),
            Self::UnknownFunction { name, .. } => format!(
                "unknown function '{name}'; available: {}",
                FUNCTION_NAMES.join(", ")
            ),
            Self::Evaluation { message } => message.clone(),
        };
        let Some(pos) = self.position() else {
            return message;
        };
        let pos = pos.min(source.len());
        // Publicly constructible expressions can carry a non-boundary offset.
        let pos = source.floor_char_boundary(pos);
        let before = &source[..pos];
        let line = before.bytes().filter(|byte| *byte == b'\n').count() + 1;
        let line_start = before.rfind('\n').map_or(0, |index| index + 1);
        let column = before[line_start..].chars().count() + 1;
        let line_end = source[pos..]
            .find('\n')
            .map_or(source.len(), |offset| pos + offset);
        let line_text = &source[line_start..line_end];
        let line_chars = line_text.chars().count();
        let excerpt_start = column.saturating_sub(61).min(line_chars);
        let excerpt_end = (excerpt_start + 121).min(line_chars);
        let excerpt = line_text
            .chars()
            .skip(excerpt_start)
            .take(excerpt_end - excerpt_start)
            .collect::<String>();
        let caret_offset = column - 1 - excerpt_start;
        let gutter = line.to_string().len();
        format!(
            "{message}\n  --> line {line}, column {column}\n{:gutter$} | {}{}{}\n{:gutter$} | {}^",
            "",
            if excerpt_start > 0 { "…" } else { "" },
            excerpt,
            if excerpt_end < line_chars { "…" } else { "" },
            "",
            " ".repeat(caret_offset)
        )
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
            Self::UnknownFunction { name, pos } => write!(
                formatter,
                "unknown function '{name}' (position {pos}); available: {}",
                FUNCTION_NAMES.join(", ")
            ),
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
                // Scientific notation is only consumed when the exponent has at
                // least one digit: `2e` stays `2 * e` (the Euler constant).
                if end < bytes.len() && matches!(bytes[end], b'e' | b'E') {
                    let mut probe = end + 1;
                    if probe < bytes.len() && matches!(bytes[probe], b'+' | b'-') {
                        probe += 1;
                    }
                    let digits = probe;
                    while probe < bytes.len() && bytes[probe].is_ascii_digit() {
                        probe += 1;
                    }
                    if probe > digits {
                        end = probe;
                    }
                }
                let text = &input[index..end];
                let value = text
                    .parse::<f64>()
                    .map_err(|_| MathError::syntax("invalid number", Some(pos)))?;
                if !value.is_finite() {
                    return Err(MathError::syntax("number is out of range", Some(pos)));
                }
                index = end;
                TokenKind::Number(value)
            }
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                let mut end = index + 1;
                while end < bytes.len() {
                    if bytes[end].is_ascii_alphanumeric()
                        || bytes[end] == b'_'
                        || (bytes[end] == b'.'
                            && end + 1 < bytes.len()
                            && (bytes[end + 1].is_ascii_alphabetic() || bytes[end + 1] == b'_'))
                    {
                        end += 1;
                    } else {
                        break;
                    }
                }
                let name = input[index..end].to_owned();
                index = end;
                TokenKind::Ident(name)
            }
            b'\'' | b'"' => {
                let quote = byte;
                let mut end = index + 1;
                let mut name = String::new();
                let mut closed = false;
                while end < bytes.len() {
                    let character = input[end..].chars().next().unwrap_or('\u{fffd}');
                    let width = character.len_utf8();
                    if character == quote as char {
                        if bytes.get(end + width) == Some(&quote) {
                            name.push(character);
                            end += width * 2;
                            continue;
                        }
                        end += width;
                        closed = true;
                        break;
                    }
                    name.push(character);
                    end += width;
                }
                if !closed {
                    return Err(MathError::syntax(
                        "unterminated quoted identifier",
                        Some(pos),
                    ));
                }
                let qualified = if bytes.get(end) == Some(&b'.')
                    && bytes
                        .get(end + 1)
                        .is_some_and(|next| next.is_ascii_alphabetic() || *next == b'_')
                {
                    let mut suffix_end = end + 1;
                    while suffix_end < bytes.len()
                        && (bytes[suffix_end].is_ascii_alphanumeric() || bytes[suffix_end] == b'_')
                    {
                        suffix_end += 1;
                    }
                    name.push('.');
                    name.push_str(&input[end + 1..suffix_end]);
                    end = suffix_end;
                    true
                } else {
                    false
                };
                index = end;
                if qualified {
                    TokenKind::Ident(name)
                } else {
                    TokenKind::QuotedIdentifier(name)
                }
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
        if tokens.len() >= MAX_TOKENS {
            return Err(MathError::syntax(
                format!("expression is too complex (maximum {MAX_TOKENS} tokens)"),
                Some(pos),
            ));
        }
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
        depth: 0,
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
    depth: usize,
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
            // Adjacent primary tokens imply multiplication: 2x, 2(x+1),
            // (x+1)(x-1), 3pi. Function calls are already resolved in
            // `parse_primary`, so a following `(` here is a product.
            let (op, explicit) = match self.peek().map(|token| &token.kind) {
                Some(TokenKind::Star) => (BinaryOp::Mul, true),
                Some(TokenKind::Slash) => (BinaryOp::Div, true),
                Some(
                    TokenKind::Number(_)
                    | TokenKind::Ident(_)
                    | TokenKind::QuotedIdentifier(_)
                    | TokenKind::LParen,
                ) => (BinaryOp::Mul, false),
                _ => break,
            };
            let pos = left.pos;
            if explicit {
                self.advance();
            }
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
        if self.depth >= MAX_PARSE_DEPTH {
            let pos = self.peek().map_or(self.source_len, |token| token.pos);
            return Err(MathError::syntax(
                "expression nesting is too deep",
                Some(pos),
            ));
        }
        self.depth += 1;
        let result = self.parse_unary_inner();
        self.depth -= 1;
        result
    }

    fn parse_unary_inner(&mut self) -> Result<Expr, MathError> {
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
                    if !is_function_name(&name) {
                        return Err(MathError::UnknownFunction { name, pos });
                    }
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
                kind: TokenKind::QuotedIdentifier(name),
                pos,
            }) => {
                let (name, pos) = (name.clone(), *pos);
                self.advance();
                Ok(Expr::new(ExprKind::Variable(name), pos))
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
    eval_node(expr, variables, 0)
}

fn eval_node(expr: &Expr, variables: &[(String, f64)], depth: usize) -> Result<f64, MathError> {
    if depth >= MAX_EVAL_DEPTH {
        return Err(MathError::Evaluation {
            message: "expression is too deeply nested to evaluate".to_owned(),
        });
    }
    let value = eval_node_inner(expr, variables, depth)?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(MathError::Evaluation {
            message: "non-finite result".to_owned(),
        })
    }
}

fn eval_node_inner(
    expr: &Expr,
    variables: &[(String, f64)],
    depth: usize,
) -> Result<f64, MathError> {
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
            let value = eval_node(value, variables, depth + 1)?;
            match op {
                UnaryOp::Plus => Ok(value),
                UnaryOp::Minus => Ok(-value),
            }
        }
        ExprKind::Binary { op, left, right } => {
            let left = eval_node(left, variables, depth + 1)?;
            let right = eval_node(right, variables, depth + 1)?;
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
                .map(|arg| eval_node(arg, variables, depth + 1))
                .collect::<Result<Vec<_>, _>>()?;
            call(name, &values, expr.pos)
        }
    }
}

fn call(name: &str, args: &[f64], pos: usize) -> Result<f64, MathError> {
    let problem = |message: String| MathError::Syntax {
        message,
        pos: Some(pos),
    };
    let exact = |expected: usize| {
        if args.len() == expected {
            Ok(())
        } else {
            Err(problem(format!(
                "{name} expects {expected} argument(s), got {}",
                args.len()
            )))
        }
    };
    let one = || {
        exact(1)?;
        Ok(args[0])
    };
    let two = || {
        exact(2)?;
        Ok((args[0], args[1]))
    };
    let nonempty = || {
        if args.is_empty() {
            Err(problem(format!("{name} expects at least one argument")))
        } else {
            Ok(())
        }
    };
    let positive = |value: f64, function: &str| {
        if value > 0.0 {
            Ok(value)
        } else {
            Err(problem(format!(
                "{function} requires a positive argument, got {}",
                format_value(value)
            )))
        }
    };
    let unit_interval = |value: f64, function: &str| {
        if (-1.0..=1.0).contains(&value) {
            Ok(value)
        } else {
            Err(problem(format!(
                "{function} requires an argument between -1 and 1, got {}",
                format_value(value)
            )))
        }
    };

    match name.to_ascii_lowercase().as_str() {
        "sin" => Ok(one()?.sin()),
        "cos" => Ok(one()?.cos()),
        "tan" => Ok(one()?.tan()),
        "asin" => Ok(unit_interval(one()?, "asin")?.asin()),
        "acos" => Ok(unit_interval(one()?, "acos")?.acos()),
        "atan" => Ok(one()?.atan()),
        "atan2" => {
            let (y, x) = two()?;
            Ok(y.atan2(x))
        }
        "sqrt" => {
            let value = one()?;
            if value < 0.0 {
                Err(problem(format!(
                    "sqrt requires a non-negative argument, got {}",
                    format_value(value)
                )))
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
                    Err(problem("log base must not be 1".to_owned()))
                } else {
                    Ok(value.log(base))
                }
            }
            other => Err(problem(format!(
                "log expects 1 or 2 arguments, got {other}"
            ))),
        },
        "pow" => {
            let (base, exponent) = two()?;
            let value = base.powf(exponent);
            if value.is_finite() {
                Ok(value)
            } else {
                Err(problem(format!(
                    "pow is undefined for {} and {}",
                    format_value(base),
                    format_value(exponent)
                )))
            }
        }
        "round" => Ok(one()?.round()),
        "floor" => Ok(one()?.floor()),
        "ceil" => Ok(one()?.ceil()),
        "trunc" => Ok(one()?.trunc()),
        "sign" => {
            let value = one()?;
            Ok(if value > 0.0 {
                1.0
            } else if value < 0.0 {
                -1.0
            } else {
                0.0
            })
        }
        "min" => {
            nonempty()?;
            Ok(args.iter().copied().fold(f64::INFINITY, f64::min))
        }
        "max" => {
            nonempty()?;
            Ok(args.iter().copied().fold(f64::NEG_INFINITY, f64::max))
        }
        "sum" => {
            nonempty()?;
            Ok(args.iter().sum())
        }
        "mean" => {
            nonempty()?;
            Ok(args.iter().sum::<f64>() / args.len() as f64)
        }
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
    if value.abs() < 1e-9 || value.abs() >= 1e15 {
        let text = format!("{value:.10e}");
        let Some((mantissa, exponent)) = text.split_once('e') else {
            return text;
        };
        let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
        let exponent = exponent.parse::<i32>().unwrap_or(0);
        return format!("{mantissa}e{exponent}");
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

    fn close(expression: &str, expected: f64) {
        close_with(expression, &[], expected);
    }

    fn close_with(expression: &str, variables: &[(String, f64)], expected: f64) {
        let actual =
            evaluate(expression, variables).unwrap_or_else(|error| panic!("{expression}: {error}"));
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
    fn scientific_notation_and_implicit_multiplication() {
        let variables = vec![("x".to_owned(), 3.0), ("r".to_owned(), 3.0)];
        close("1e3", 1000.0);
        close("2.5e-2", 0.025);
        close("3E+1", 30.0);
        close("2e", 2.0 * std::f64::consts::E);
        close_with("2x", &variables, 6.0);
        close_with("2(x+1)", &variables, 8.0);
        close_with("(x+1)(x-1)", &variables, 8.0);
        close_with("pi r^2", &variables, std::f64::consts::PI * 9.0);
        close("2sin(pi/2)", 2.0);
    }

    #[test]
    fn additional_functions_and_precise_errors() {
        close("asin(0)", 0.0);
        close("acos(1)", 0.0);
        close("atan(1)", std::f64::consts::FRAC_PI_4);
        close("atan2(1,1)", std::f64::consts::FRAC_PI_4);
        close("round(2.6)", 3.0);
        close("floor(-1.2)", -2.0);
        close("ceil(1.2)", 2.0);
        close("trunc(-1.9)", -1.0);
        close("sign(-4)", -1.0);
        close("min(3,1,2)", 1.0);
        close("max(3,1,2)", 3.0);
        close("sum(1,2,3)", 6.0);
        close("mean(1,2,3,4)", 2.5);
        close("pow(2,10)", 1024.0);

        let error = evaluate("asin(2)", &[]).unwrap_err().to_string();
        assert!(error.contains("between -1 and 1"), "{error}");
        assert!(error.contains("got 2"), "{error}");
        let error = evaluate("sqrt(-4)", &[]).unwrap_err().to_string();
        assert!(
            error.contains("non-negative") && error.contains("-4"),
            "{error}"
        );
        let error = evaluate("min()", &[]).unwrap_err().to_string();
        assert!(error.contains("at least one argument"), "{error}");
        let error = evaluate("foo(1)", &[]).unwrap_err().to_string();
        assert!(error.contains("available:"), "{error}");
    }

    #[test]
    fn invalid_syntax_is_actionable() {
        for input in ["2+", "(2+3", "2 + * 3", "sqrt()", "2..3", "1 $ 2"] {
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
        assert_eq!(format_value(1.25e-12), "1.25e-12");
        assert_eq!(format_value(1.0e20), "1e20");
    }

    #[test]
    fn qualified_and_quoted_sheet_names_tokenize_as_variables() {
        let variables = vec![
            ("Plot.A1".to_owned(), 6.0),
            ("My Sheet.A1".to_owned(), 8.0),
            ("My Sheet".to_owned(), 10.0),
        ];
        close_with("Plot.A1 + 1", &variables, 7.0);
        close_with("'My Sheet'.A1 * 2", &variables, 16.0);
        close_with("\"My Sheet\" + 1", &variables, 11.0);
        close_with("'O''Brien'.A1", &[("O'Brien.A1".to_owned(), 4.0)], 4.0);
        assert!(evaluate("'unterminated", &[]).is_err());
    }

    #[test]
    fn diagnostics_include_line_column_and_a_caret() {
        let error = evaluate("x + sqrt(-1)", &[("x".to_owned(), 1.0)]).unwrap_err();
        let diagnostic = error.diagnostic("x + sqrt(-1)");
        assert!(diagnostic.contains("line 1, column 5"), "{diagnostic}");
        assert!(diagnostic.contains("sqrt(-1)"));
        assert!(diagnostic.contains('^'));

        let error = evaluate("'é' + @", &[]).unwrap_err();
        let diagnostic = error.diagnostic("'é' + @");
        assert!(diagnostic.contains("column 7"), "{diagnostic}");

        let error = evaluate("1+\n@", &[]).unwrap_err();
        let diagnostic = error.diagnostic("1+\n@");
        assert!(diagnostic.contains("line 2, column 1"), "{diagnostic}");
    }

    #[test]
    fn malformed_scientific_notation_and_overflow_are_errors() {
        for input in ["1e+", "1e-", "1e9999", ".", "1.2.3"] {
            assert!(evaluate(input, &[]).is_err(), "{input}");
        }
        close("1.5E-3", 0.0015);
        close(".5 + 5.", 5.5);
        close_with("1e-3x", &[("x".to_owned(), 2.0)], 0.002);
    }

    #[test]
    fn external_input_cannot_exhaust_the_parser_or_evaluator_stack() {
        let nested = format!("{}1{}", "(".repeat(300), ")".repeat(300));
        assert!(evaluate(&nested, &[]).is_err());
        let signs = "-".repeat(300) + "1";
        assert!(evaluate(&signs, &[]).is_err());
        let long = vec!["1"; MAX_TOKENS + 1].join("+");
        assert!(evaluate(&long, &[]).is_err());
        let bounded = vec!["1"; 300].join("+");
        close(&bounded, 300.0);
    }

    #[test]
    fn non_finite_inputs_and_results_are_rejected() {
        assert!(evaluate("x", &[("x".to_owned(), f64::NAN)]).is_err());
        assert!(evaluate("min(1,x)", &[("x".to_owned(), f64::NAN)]).is_err());
        assert!(evaluate("exp(10000)", &[]).is_err());
        assert!(evaluate("1e308*1e308", &[]).is_err());
    }
}
