// SPDX-License-Identifier: GPL-3.0-or-later
//! Recursive-descent parser backing [`crate::expr::eval`] — split out of
//! `expr.rs` for file-size cohesion (see that module for the public API,
//! grammar reference, and the ADR-0006 write-fresh rationale).

use crate::expr::{ExprError, MAX_DEPTH};
use std::iter::Peekable;
use std::str::CharIndices;

/// A minimal, on-demand ("lazy") tokenizer + recursive-descent parser.
///
/// Tokens are read from the source one at a time as the grammar needs them,
/// rather than up front. This matters for function-call arguments such as
/// `table(x, "f.inc")`: because those forms are rejected as
/// [`ExprError::UnsupportedFn`] as soon as `identifier` `(` is recognized,
/// the string literal inside never needs to be lexed at all — the grammar
/// has no string literal support otherwise.
pub(crate) struct Parser<'a> {
    src: &'a str,
    chars: Peekable<CharIndices<'a>>,
    lookup: &'a dyn Fn(&str) -> Option<f64>,
    depth: usize,
}

impl<'a> Parser<'a> {
    pub(crate) fn new(src: &'a str, lookup: &'a dyn Fn(&str) -> Option<f64>) -> Self {
        Self {
            src,
            chars: src.char_indices().peekable(),
            lookup,
            depth: 0,
        }
    }

    pub(crate) fn pos(&mut self) -> usize {
        self.chars.peek().map_or(self.src.len(), |&(i, _)| i)
    }

    pub(crate) fn peek_char(&mut self) -> Option<char> {
        self.skip_whitespace();
        self.chars.peek().map(|&(_, c)| c)
    }

    pub(crate) fn skip_whitespace(&mut self) {
        while matches!(self.chars.peek(), Some((_, c)) if c.is_whitespace()) {
            self.chars.next();
        }
    }

    /// Consumes `text` if the upcoming (whitespace-skipped) input starts
    /// with it, returning whether it matched.
    fn eat_str(&mut self, text: &str) -> bool {
        self.skip_whitespace();
        let rest = &self.src[self.pos()..];
        if rest.starts_with(text) {
            for _ in 0..text.chars().count() {
                self.chars.next();
            }
            true
        } else {
            false
        }
    }

    fn eat_char(&mut self, c: char) -> bool {
        self.skip_whitespace();
        if self.chars.peek().map(|&(_, ch)| ch) == Some(c) {
            self.chars.next();
            true
        } else {
            false
        }
    }

    /// Enters one level of recursive parsing, enforcing [`MAX_DEPTH`].
    fn enter(&mut self) -> Result<(), ExprError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(ExprError::TooDeep);
        }
        Ok(())
    }

    fn exit(&mut self) {
        self.depth -= 1;
    }

    // expr := or_expr
    pub(crate) fn parse_expr(&mut self) -> Result<f64, ExprError> {
        self.enter()?;
        let result = self.parse_or();
        self.exit();
        result
    }

    // or_expr := and_expr ( "||" and_expr )*
    //
    // Evaluated eagerly (both sides always evaluated), not short-circuited.
    // See [`crate::expr`]'s module doc for the rationale: this is a
    // single-pass parser/evaluator, and a config-condition evaluator should
    // fail loud on a typo'd variable regardless of which side of `&&`/`||`
    // it lands on.
    fn parse_or(&mut self) -> Result<f64, ExprError> {
        let mut lhs = self.parse_and()?;
        loop {
            self.skip_whitespace();
            if self.eat_str("||") {
                let rhs = self.parse_and()?;
                lhs = bool_to_f64(truthy(lhs) || truthy(rhs));
            } else {
                break;
            }
        }
        Ok(lhs)
    }

    // and_expr := compare ( "&&" compare )*
    //
    // Evaluated eagerly — see [`Parser::parse_or`].
    fn parse_and(&mut self) -> Result<f64, ExprError> {
        let mut lhs = self.parse_compare()?;
        loop {
            self.skip_whitespace();
            if self.eat_str("&&") {
                let rhs = self.parse_compare()?;
                lhs = bool_to_f64(truthy(lhs) && truthy(rhs));
            } else {
                break;
            }
        }
        Ok(lhs)
    }

    // compare := additive ( ("==" | "!=" | "<=" | ">=" | "<" | ">") additive )*
    fn parse_compare(&mut self) -> Result<f64, ExprError> {
        let mut lhs = self.parse_additive()?;
        loop {
            self.skip_whitespace();
            let op = if self.eat_str("==") {
                Some(CompareOp::Eq)
            } else if self.eat_str("!=") {
                Some(CompareOp::Ne)
            } else if self.eat_str("<=") {
                Some(CompareOp::Le)
            } else if self.eat_str(">=") {
                Some(CompareOp::Ge)
            } else if self.eat_str("<") {
                Some(CompareOp::Lt)
            } else if self.eat_str(">") {
                Some(CompareOp::Gt)
            } else {
                None
            };
            match op {
                Some(op) => {
                    let rhs = self.parse_additive()?;
                    lhs = op.apply(lhs, rhs);
                }
                None => break,
            }
        }
        Ok(lhs)
    }

    // additive := term ( ("+" | "-") term )*
    fn parse_additive(&mut self) -> Result<f64, ExprError> {
        let mut lhs = self.parse_term()?;
        loop {
            self.skip_whitespace();
            if self.eat_char('+') {
                lhs += self.parse_term()?;
            } else if self.eat_char('-') {
                lhs -= self.parse_term()?;
            } else {
                break;
            }
        }
        Ok(lhs)
    }

    // term := unary ( ("*" | "/") unary )*
    fn parse_term(&mut self) -> Result<f64, ExprError> {
        let mut lhs = self.parse_unary()?;
        loop {
            self.skip_whitespace();
            if self.eat_char('*') {
                lhs *= self.parse_unary()?;
            } else if self.eat_char('/') {
                let rhs = self.parse_unary()?;
                if rhs == 0.0 {
                    return Err(ExprError::Math);
                }
                lhs /= rhs;
            } else {
                break;
            }
        }
        Ok(lhs)
    }

    // unary := "!" unary | "-" unary | primary
    fn parse_unary(&mut self) -> Result<f64, ExprError> {
        self.enter()?;
        self.skip_whitespace();
        let result = if self.eat_char('!') {
            let val = self.parse_unary();
            val.map(|v| bool_to_f64(!truthy(v)))
        } else if self.eat_char('-') {
            self.parse_unary().map(|v| -v)
        } else {
            self.parse_primary()
        };
        self.exit();
        result
    }

    // primary := number | ident | ident "(" ... ")" | "(" expr ")"
    fn parse_primary(&mut self) -> Result<f64, ExprError> {
        self.skip_whitespace();
        if self.eat_char('(') {
            self.enter()?;
            let value = self.parse_or();
            self.exit();
            let value = value?;
            self.skip_whitespace();
            if !self.eat_char(')') {
                return Err(ExprError::Syntax("expected `)`".to_string()));
            }
            return Ok(value);
        }

        match self.peek_char() {
            Some(c) if c.is_ascii_digit() || c == '.' => self.parse_number(),
            Some(c) if is_ident_start(c) => self.parse_ident_or_call(),
            Some(c) => Err(ExprError::Syntax(format!("unexpected character `{c}`"))),
            None => Err(ExprError::Syntax(
                "unexpected end of expression".to_string(),
            )),
        }
    }

    fn parse_number(&mut self) -> Result<f64, ExprError> {
        let start = self.pos();
        let mut saw_dot = false;
        while let Some((_, c)) = self.chars.peek().copied() {
            if c.is_ascii_digit() {
                self.chars.next();
            } else if c == '.' && !saw_dot {
                saw_dot = true;
                self.chars.next();
            } else {
                break;
            }
        }
        let end = self.pos();
        self.src[start..end]
            .parse::<f64>()
            .map_err(|_| ExprError::Syntax(format!("invalid number literal at byte {start}")))
    }

    fn parse_ident(&mut self) -> String {
        let start = self.pos();
        while let Some((_, c)) = self.chars.peek().copied() {
            if is_ident_continue(c) {
                self.chars.next();
            } else {
                break;
            }
        }
        let end = self.pos();
        self.src[start..end].to_string()
    }

    /// Parses a bare identifier, or — if immediately followed by `(` — a
    /// function-call form. All function calls are currently unsupported: as
    /// soon as `name(` is recognized we return [`ExprError::UnsupportedFn`]
    /// without attempting to lex the argument list (which may contain
    /// syntax this grammar does not otherwise support, e.g. string
    /// literals).
    fn parse_ident_or_call(&mut self) -> Result<f64, ExprError> {
        let name = self.parse_ident();
        self.skip_whitespace();
        if self.chars.peek().map(|&(_, c)| c) == Some('(') {
            return Err(ExprError::UnsupportedFn(name));
        }
        (self.lookup)(&name).ok_or(ExprError::UnknownVar(name))
    }
}

enum CompareOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl CompareOp {
    fn apply(&self, lhs: f64, rhs: f64) -> f64 {
        let result = match self {
            CompareOp::Eq => lhs == rhs,
            CompareOp::Ne => lhs != rhs,
            CompareOp::Lt => lhs < rhs,
            CompareOp::Le => lhs <= rhs,
            CompareOp::Gt => lhs > rhs,
            CompareOp::Ge => lhs >= rhs,
        };
        bool_to_f64(result)
    }
}

fn truthy(value: f64) -> bool {
    value != 0.0
}

fn bool_to_f64(value: bool) -> f64 {
    if value {
        1.0
    } else {
        0.0
    }
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_continue(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}
