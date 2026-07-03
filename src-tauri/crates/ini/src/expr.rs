// SPDX-License-Identifier: GPL-3.0-or-later
//! Sandboxed expression evaluator — sub-steps 2.1–2.4.
//!
//! Real INIs put small arithmetic/boolean expressions where a plain number
//! or condition is expected: constant scaling (`{ 0.1 / stoich }`) and
//! dialog `visible`/`enable` guards (`injLayout != 0 && nCylinders == 4`).
//! This module evaluates that closed grammar against a caller-supplied
//! variable lookup. The recursive-descent parser itself lives in
//! [`crate::expr_parser`] (split out for file-size cohesion); this module
//! is the public surface, the error type, and the license note.
//!
//! # Sandboxing guarantee
//!
//! This is **not** a general-purpose `eval` — there is no code execution, no
//! I/O, no environment access, and no user-definable functions. The grammar
//! is a small fixed set of arithmetic/comparison/boolean operators plus
//! literals and bare-symbol variable references resolved through a caller
//! `&dyn Fn(&str) -> Option<f64>` closure. Parsing recursion is bounded by a
//! fixed depth cap to prevent stack overflow on pathological input (e.g.
//! very deeply nested parentheses) — see [`ExprError::TooDeep`].
//!
//! # Write-fresh exception (ADR-0006)
//!
//! Per [ADR-0006](../../../docs/adr/0006-reuse-existing-parsers.md), this
//! crate defaults to *porting* a proven open reference. The only working
//! reference for this grammar is rusEFI's `ExpressionEvaluator.java`, which
//! is GPLv3 with additional §7 "field of use" terms (restricting use in
//! aircraft/off-road applications) — terms this project does not carry
//! elsewhere and should not inherit via a port. hyper-tuner and
//! `adbancroft/TunerStudioIniParser` both store `{ … }` constant expressions
//! as opaque strings and never evaluate them, so neither is a usable
//! reference either.
//!
//! **This module is therefore written fresh** — a deliberate, recorded
//! exception to the ADR-0006 default. rusEFI's evaluator was consulted only
//! as a *structural* reference (confirming the operator set actually used
//! in real INIs — see the grammar below) — no code or algorithm was copied
//! from it. See also the `Note (2026-07-02)` entry in ADR-0006 itself.
//!
//! # Grammar (precedence, loosest to tightest)
//!
//! ```text
//! expr       := or_expr
//! or_expr    := and_expr ( "||" and_expr )*
//! and_expr   := compare ( "&&" compare )*
//! compare    := additive ( ("==" | "!=" | "<=" | ">=" | "<" | ">") additive )*
//! additive   := term ( ("+" | "-") term )*
//! term       := unary ( ("*" | "/") unary )*
//! unary      := "!" unary | "-" unary | primary
//! primary    := number | ident | ident "(" ... ")" | "(" expr ")"
//! ```
//!
//! Booleans are represented as `f64`: `1.0` for true, `0.0` for false (any
//! non-zero value is truthy on input, per [`eval_bool`]).
//!
//! `&&` and `||` are evaluated **eagerly** — both sides are always
//! evaluated, unlike C/Rust's short-circuiting `&&`/`||`. This is a
//! deliberate choice (the grammar itself mandates neither): this is a
//! single-pass parser/evaluator with no separate AST stage, so
//! short-circuiting would mean discarding errors produced by a right-hand
//! side that was only partially *parsed* (its tokens must still be
//! consumed for the rest of the expression to parse correctly), which
//! risks silently swallowing genuine syntax errors along with the intended
//! "unevaluated" ones. Eager evaluation also fails loud on a typo'd
//! variable regardless of which side of `&&`/`||` it lands on — e.g.
//! `injLayout == 0 && bogusVar == 4` reports `UnknownVar("bogusVar")` even
//! though `injLayout == 0` is false.

use crate::expr_parser::Parser;

/// Recursion depth cap for parsing (matches expression nesting depth, e.g.
/// parenthesised sub-expressions). Prevents stack overflow on pathological
/// input; real INI expressions never nest anywhere close to this deep.
pub(crate) const MAX_DEPTH: usize = 64;

/// Errors produced while evaluating a sandboxed expression.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ExprError {
    /// A bare-symbol variable reference that the lookup closure could not
    /// resolve. Carries the unresolved name.
    #[error("unknown variable: `{0}`")]
    UnknownVar(String),
    /// A recognized function-call form with no evaluator support (e.g.
    /// `bitStringValue(...)`, `table(...)`). Carries the function name.
    #[error("unsupported function: `{0}`")]
    UnsupportedFn(String),
    /// A runtime arithmetic error (currently: division by zero).
    #[error("arithmetic error (division by zero)")]
    Math,
    /// Parsing recursed past the depth cap (e.g. pathologically nested
    /// parentheses).
    #[error("expression nested too deeply (max {})", MAX_DEPTH)]
    TooDeep,
    /// The input did not conform to the grammar (unexpected/missing token,
    /// trailing input, unbalanced parens, empty expression, ...). Carries a
    /// human-readable detail for diagnostics.
    #[error("syntax error: {0}")]
    Syntax(String),
}

/// Evaluates `expr` to a numeric result, resolving bare-symbol variable
/// references via `lookup`.
///
/// # Errors
///
/// Returns [`ExprError`] on an unknown variable, an unsupported function
/// call, a division by zero, a malformed expression, or excessive nesting.
pub fn eval(expr: &str, lookup: &dyn Fn(&str) -> Option<f64>) -> Result<f64, ExprError> {
    let mut parser = Parser::new(expr, lookup);
    let value = parser.parse_expr()?;
    parser.skip_whitespace();
    if parser.peek_char().is_some() {
        return Err(ExprError::Syntax(format!(
            "unexpected trailing input at byte {}",
            parser.pos()
        )));
    }
    Ok(value)
}

/// Evaluates `expr` as a boolean condition: any non-zero numeric result is
/// `true`. Used for `visible`/`enable` dialog conditions.
///
/// # Errors
///
/// See [`eval`].
pub fn eval_bool(expr: &str, lookup: &dyn Fn(&str) -> Option<f64>) -> Result<bool, ExprError> {
    Ok(eval(expr, lookup)? != 0.0)
}
