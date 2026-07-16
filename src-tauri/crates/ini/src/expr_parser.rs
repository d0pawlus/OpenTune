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
    /// Function-call resolver: `(name, args) -> value`. `None` means the
    /// function is unsupported — plain [`crate::eval`] passes a resolver
    /// that always returns `None`.
    funcs: &'a dyn Fn(&str, &[f64]) -> Option<f64>,
    depth: usize,
    /// Inside the UNTAKEN branch of a ternary: name/function resolution
    /// failures yield 0.0 instead of erroring (the branch still parses for
    /// grammar, its value is discarded). See [`Parser::parse_ternary`].
    muted: bool,
}

/// The no-function resolver used by plain [`crate::eval`].
pub(crate) fn no_functions(_name: &str, _args: &[f64]) -> Option<f64> {
    None
}

impl<'a> Parser<'a> {
    pub(crate) fn new(
        src: &'a str,
        lookup: &'a dyn Fn(&str) -> Option<f64>,
        funcs: &'a dyn Fn(&str, &[f64]) -> Option<f64>,
    ) -> Self {
        Self {
            src,
            chars: src.char_indices().peekable(),
            lookup,
            funcs,
            depth: 0,
            muted: false,
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

    // expr := ternary
    pub(crate) fn parse_expr(&mut self) -> Result<f64, ExprError> {
        self.enter()?;
        let result = self.parse_ternary();
        self.exit();
        result
    }

    // ternary := or_expr ( "?" expr ":" expr )?
    //
    // Lowest precedence and right-associative, per C. MS3-era INIs compute
    // bounds and scale factors with it (`{ clt_exp ? 230 : 120 }`).
    //
    // Branches recurse through [`Parser::parse_expr`] so the `MAX_DEPTH`
    // guard applies — a ternary nesting bomb degrades to `TooDeep` instead
    // of overflowing the stack (PR #22 review finding).
    //
    // Unlike `&&`/`||` (eager — see [`Parser::parse_or`]), only the TAKEN
    // branch's resolution errors surface: TunerStudio evaluates `?:`
    // lazily, and real tunes rely on it (`curve == 0 ? 1 :
    // getChannelScaleByOffset(offset)` with an unconfigured PWM slot must
    // yield 1, not fail the constant). The untaken branch still parses —
    // the grammar needs its extent — with resolution failures muted to 0.0
    // (see [`Parser::muted`]); its syntax errors still surface.
    fn parse_ternary(&mut self) -> Result<f64, ExprError> {
        let cond = self.parse_or()?;
        self.skip_whitespace();
        if !self.eat_char('?') {
            return Ok(cond);
        }
        let outer = self.muted;
        let take_then = truthy(cond);
        self.muted = outer || !take_then;
        let then_value = self.parse_expr();
        self.muted = outer;
        let then_value = then_value?;
        self.skip_whitespace();
        if !self.eat_char(':') {
            return Err(ExprError::Syntax("expected `:` in ternary".to_string()));
        }
        self.muted = outer || take_then;
        let else_value = self.parse_expr();
        self.muted = outer;
        let else_value = else_value?;
        Ok(if take_then { then_value } else { else_value })
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

    // and_expr := bitand ( "&&" bitand )*
    //
    // Evaluated eagerly — see [`Parser::parse_or`].
    fn parse_and(&mut self) -> Result<f64, ExprError> {
        let mut lhs = self.parse_bitand()?;
        loop {
            self.skip_whitespace();
            if self.eat_str("&&") {
                let rhs = self.parse_bitand()?;
                lhs = bool_to_f64(truthy(lhs) && truthy(rhs));
            } else {
                break;
            }
        }
        Ok(lhs)
    }

    // bitand := compare ( "&" compare )*
    //
    // Bitwise AND on f64 values cast through i64 (M3 Task 3 — real
    // indicators use it: `{ sd_status & 1 }`). Sits between equality and
    // `&&`, per standard C precedence. Only a *single* `&` is consumed
    // here — `&&` belongs to [`Parser::parse_and`] above (see
    // [`Parser::eat_single_amp`]).
    fn parse_bitand(&mut self) -> Result<f64, ExprError> {
        let mut lhs = self.parse_compare()?;
        while self.eat_single_amp() {
            let rhs = self.parse_compare()?;
            lhs = ((lhs as i64) & (rhs as i64)) as f64;
        }
        Ok(lhs)
    }

    /// Consumes a single `&` only when it is *not* the start of `&&`
    /// (which must stay intact for [`Parser::parse_and`] to see).
    fn eat_single_amp(&mut self) -> bool {
        self.skip_whitespace();
        let rest = &self.src[self.pos()..];
        if rest.starts_with('&') && !rest.starts_with("&&") {
            self.chars.next();
            true
        } else {
            false
        }
    }

    // compare := shift ( ("==" | "!=" | "<=" | ">=" | "<" | ">") shift )*
    //
    // Operands are shift-expressions: `<<` binds tighter than any
    // comparison (standard C precedence). `parse_shift` consumes every
    // `<<` greedily before returning, so the `<`/`<=` checks below can
    // never mistake the first half of a `<<` for a comparison.
    fn parse_compare(&mut self) -> Result<f64, ExprError> {
        let mut lhs = self.parse_shift()?;
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
                    let rhs = self.parse_shift()?;
                    lhs = op.apply(lhs, rhs);
                }
                None => break,
            }
        }
        Ok(lhs)
    }

    // shift := additive ( "<<" additive )*
    //
    // Bitwise left shift on f64 values cast through i64 (M3 Task 3 — real
    // computed channels use it: `{ halfSync + (sync << 1) }`). Binds
    // tighter than comparison, looser than `+`/`-`, per standard C
    // precedence. A shift count outside `0..=63` is undefined for i64 —
    // it degrades loudly as [`ExprError::Math`], like division by zero.
    fn parse_shift(&mut self) -> Result<f64, ExprError> {
        let mut lhs = self.parse_additive()?;
        loop {
            self.skip_whitespace();
            if self.eat_str("<<") {
                let rhs = self.parse_additive()?;
                let amount = rhs as i64;
                if !(0..64).contains(&amount) {
                    return Err(ExprError::Math);
                }
                lhs = ((lhs as i64) << amount) as f64;
            } else {
                break;
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

    // unary := "!" unary | "+" unary | "-" unary | primary
    fn parse_unary(&mut self) -> Result<f64, ExprError> {
        self.enter()?;
        self.skip_whitespace();
        let result = if self.eat_char('!') {
            let val = self.parse_unary();
            val.map(|v| bool_to_f64(!truthy(v)))
        } else if self.eat_char('+') {
            self.parse_unary()
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
        let mut saw_digit = false;

        while matches!(self.chars.peek(), Some((_, c)) if c.is_ascii_digit()) {
            saw_digit = true;
            self.chars.next();
        }
        if matches!(self.chars.peek(), Some((_, '.'))) {
            self.chars.next();
            while matches!(self.chars.peek(), Some((_, c)) if c.is_ascii_digit()) {
                saw_digit = true;
                self.chars.next();
            }
        }
        if !saw_digit {
            return Err(ExprError::Syntax(format!(
                "invalid number literal at byte {start}"
            )));
        }

        if matches!(self.chars.peek(), Some((_, 'e' | 'E'))) {
            self.chars.next();
            if matches!(self.chars.peek(), Some((_, '+' | '-'))) {
                self.chars.next();
            }
            let mut saw_exponent_digit = false;
            while matches!(self.chars.peek(), Some((_, c)) if c.is_ascii_digit()) {
                saw_exponent_digit = true;
                self.chars.next();
            }
            if !saw_exponent_digit {
                return Err(ExprError::Syntax(format!(
                    "invalid exponent at byte {start}"
                )));
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
    /// function-call form. Each argument is a full expression; the call is
    /// resolved via the `funcs` hook, and a `None` from it surfaces as
    /// [`ExprError::UnsupportedFn`] (which is every call under plain
    /// [`crate::eval`] — only [`crate::eval_with_functions`] callers supply
    /// builtins, e.g. MS3's `getChannelScaleByOffset(...)`). Arguments this
    /// grammar cannot lex (e.g. string literals) stay syntax errors.
    fn parse_ident_or_call(&mut self) -> Result<f64, ExprError> {
        let name = self.parse_ident();
        self.skip_whitespace();
        if self.chars.peek().map(|&(_, c)| c) != Some('(') {
            return match (self.lookup)(&name) {
                Some(value) => Ok(value),
                None if self.muted => Ok(0.0),
                None => Err(ExprError::UnknownVar(name)),
            };
        }
        self.chars.next(); // consume '('
        let mut args = Vec::new();
        self.skip_whitespace();
        if !self.eat_char(')') {
            loop {
                args.push(self.parse_expr()?);
                if self.eat_char(',') {
                    continue;
                }
                if self.eat_char(')') {
                    break;
                }
                return Err(ExprError::Syntax(format!(
                    "expected `,` or `)` in `{name}(...)` arguments at byte {}",
                    self.pos()
                )));
            }
        }
        match (self.funcs)(&name, &args) {
            Some(value) => Ok(value),
            None if self.muted => Ok(0.0),
            None => Err(ExprError::UnsupportedFn(name)),
        }
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
