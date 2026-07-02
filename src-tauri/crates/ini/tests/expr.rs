// SPDX-License-Identifier: GPL-3.0-or-later
//! Integration tests for the sandboxed expression evaluator — sub-steps
//! 2.1–2.4.
//!
//! **Written fresh** (no port) — see the `expr` module doc comment and
//! [ADR-0006](../../../docs/adr/0006-reuse-existing-parsers.md) for the
//! license-driven rationale (rusEFI's `ExpressionEvaluator.java` is GPLv3
//! with additional field-of-use terms incompatible with this tree).

use opentune_ini::{eval, eval_bool, ExprError};

/// A lookup table for tests: resolves a fixed set of variable names, mirroring
/// how real INIs reference other constants (e.g. `injLayout`, `nCylinders`).
fn lookup(name: &str) -> Option<f64> {
    match name {
        "injLayout" => Some(4.0),
        "nCylinders" => Some(4.0),
        "stoich" => Some(14.7),
        "boostTableLimit" => Some(250.0),
        "zero" => Some(0.0),
        _ => None,
    }
}

fn no_vars(_name: &str) -> Option<f64> {
    None
}

// ---------------------------------------------------------------------
// 2.1 — literals, arithmetic, precedence, parens
// ---------------------------------------------------------------------

#[test]
fn evaluates_integer_literal() {
    assert_eq!(eval("42", &no_vars).unwrap(), 42.0);
}

#[test]
fn evaluates_float_literal() {
    assert_eq!(eval("0.1", &no_vars).unwrap(), 0.1);
}

#[test]
fn adds_two_numbers() {
    assert_eq!(eval("2 + 3", &no_vars).unwrap(), 5.0);
}

#[test]
fn subtracts_two_numbers() {
    assert_eq!(eval("5 - 3", &no_vars).unwrap(), 2.0);
}

#[test]
fn multiplies_two_numbers() {
    assert_eq!(eval("2 * 3", &no_vars).unwrap(), 6.0);
}

#[test]
fn divides_two_numbers() {
    assert_eq!(eval("6 / 2", &no_vars).unwrap(), 3.0);
}

#[test]
fn multiplication_binds_tighter_than_addition() {
    // Naive left-to-right evaluation would give 20; correct precedence gives 14.
    assert_eq!(eval("2 + 3 * 4", &no_vars).unwrap(), 14.0);
}

#[test]
fn parens_override_precedence() {
    assert_eq!(eval("(2 + 3) * 4", &no_vars).unwrap(), 20.0);
}

#[test]
fn subtraction_is_left_associative() {
    // Right-associative (or naive recursive) evaluation would give 9.
    assert_eq!(eval("10 - 3 - 2", &no_vars).unwrap(), 5.0);
}

#[test]
fn division_is_left_associative() {
    // Right-associative evaluation would give 40.
    assert_eq!(eval("100 / 5 / 2", &no_vars).unwrap(), 10.0);
}

#[test]
fn unary_minus_negates_a_literal() {
    assert_eq!(eval("-5", &no_vars).unwrap(), -5.0);
}

#[test]
fn unary_minus_combines_with_subtraction() {
    assert_eq!(eval("2 - -3", &no_vars).unwrap(), 5.0);
}

#[test]
fn nested_parens_evaluate_correctly() {
    assert_eq!(eval("((1 + 2) * (3 + 4))", &no_vars).unwrap(), 21.0);
}

// ---------------------------------------------------------------------
// 2.1 — comparisons
// ---------------------------------------------------------------------

#[test]
fn less_than_is_true() {
    assert_eq!(eval("1 < 2", &no_vars).unwrap(), 1.0);
}

#[test]
fn less_than_is_false() {
    assert_eq!(eval("2 < 1", &no_vars).unwrap(), 0.0);
}

#[test]
fn less_equal_holds_on_equality() {
    assert_eq!(eval("2 <= 2", &no_vars).unwrap(), 1.0);
}

#[test]
fn greater_than_is_true() {
    assert_eq!(eval("3 > 2", &no_vars).unwrap(), 1.0);
}

#[test]
fn greater_equal_holds_on_equality() {
    assert_eq!(eval("2 >= 2", &no_vars).unwrap(), 1.0);
}

#[test]
fn equality_is_true() {
    assert_eq!(eval("4 == 4", &no_vars).unwrap(), 1.0);
}

#[test]
fn inequality_is_true() {
    assert_eq!(eval("4 != 5", &no_vars).unwrap(), 1.0);
}

#[test]
fn arithmetic_binds_tighter_than_comparison() {
    // Covers both `*` > `+` and arithmetic > `==` in one expression.
    assert_eq!(eval("2 + 3 * 4 == 14", &no_vars).unwrap(), 1.0);
}

// ---------------------------------------------------------------------
// 2.1 — boolean operators
// ---------------------------------------------------------------------

#[test]
fn logical_and_true_true() {
    assert_eq!(eval("1 && 1", &no_vars).unwrap(), 1.0);
}

#[test]
fn logical_and_short_circuits_to_false() {
    assert_eq!(eval("1 && 0", &no_vars).unwrap(), 0.0);
}

#[test]
fn logical_or_true() {
    assert_eq!(eval("0 || 1", &no_vars).unwrap(), 1.0);
}

#[test]
fn logical_not_negates_truthy() {
    assert_eq!(eval("!1", &no_vars).unwrap(), 0.0);
}

#[test]
fn logical_not_negates_falsy() {
    assert_eq!(eval("!0", &no_vars).unwrap(), 1.0);
}

#[test]
fn not_binds_tighter_than_and() {
    // If `!` bound looser than `&&`, this would parse as `!(1 && 0)` = 1.
    // Correct precedence parses `(!1) && 0` = 0.
    assert_eq!(eval("!1 && 0", &no_vars).unwrap(), 0.0);
}

#[test]
fn and_binds_tighter_than_or() {
    // If `||` bound tighter, this would parse as `1 || (0 && 0)` still = 1
    // either way for this case's *result*, so use a case that discriminates:
    // `0 || 1 && 0` -> `0 || (1 && 0)` = `0 || 0` = 0.
    // Whereas `(0 || 1) && 0` = `1 && 0` = 0 too — pick a genuinely
    // discriminating case instead:
    assert_eq!(eval("1 || 0 && 0", &no_vars).unwrap(), 1.0);
    // `1 || (0 && 0)` = `1 || 0` = 1 (correct precedence).
    // `(1 || 0) && 0` = `1 && 0` = 0 (wrong precedence would give this).
}

#[test]
fn nonzero_comparison_result_feeds_boolean_ops() {
    assert_eq!(eval("(1 < 2) && (3 > 2)", &no_vars).unwrap(), 1.0);
}

// ---------------------------------------------------------------------
// 2.1 — variable lookup
// ---------------------------------------------------------------------

#[test]
fn resolves_bare_variable_via_lookup() {
    assert_eq!(eval("injLayout", &lookup).unwrap(), 4.0);
}

#[test]
fn real_ini_style_condition_evaluates_true() {
    // From real speeduino.ini dialog `enable`/`visible` conditions.
    assert_eq!(
        eval("injLayout != 0 && nCylinders == 4", &lookup).unwrap(),
        1.0
    );
}

#[test]
fn real_ini_style_condition_evaluates_false() {
    assert_eq!(
        eval("injLayout == 0 && nCylinders == 4", &lookup).unwrap(),
        0.0
    );
}

#[test]
fn resolves_variable_used_in_arithmetic() {
    // `0.1 / stoich` style constant scale expression.
    assert_eq!(eval("0.1 / stoich", &lookup).unwrap(), 0.1 / 14.7);
}

#[test]
fn unknown_variable_is_an_error() {
    let err = eval("unknownSymbol", &no_vars).unwrap_err();
    assert_eq!(err, ExprError::UnknownVar("unknownSymbol".to_string()));
}

#[test]
fn unknown_variable_inside_larger_expression_is_an_error() {
    // LHS (`injLayout != 0`) is true; RHS still errors on `bogusVar`.
    let err = eval("injLayout != 0 && bogusVar == 4", &lookup).unwrap_err();
    assert_eq!(err, ExprError::UnknownVar("bogusVar".to_string()));
}

#[test]
fn and_evaluates_eagerly_rhs_unknown_var_surfaces_even_when_lhs_false() {
    // `&&`/`||` are eager, not short-circuiting (see the `expr` module
    // doc): even though `injLayout == 0` is false — which would make a
    // short-circuiting `&&` skip the right side entirely — the unresolved
    // `bogusVar` on the right must still surface as an error. This is the
    // discriminating case: a short-circuiting evaluator would instead
    // return `Ok(0.0)` here.
    let err = eval("injLayout == 0 && bogusVar == 4", &lookup).unwrap_err();
    assert_eq!(err, ExprError::UnknownVar("bogusVar".to_string()));
}

// ---------------------------------------------------------------------
// 2.2 — eval_bool
// ---------------------------------------------------------------------

#[test]
fn eval_bool_treats_nonzero_as_true() {
    assert!(eval_bool("1", &no_vars).unwrap());
    assert!(eval_bool("42", &no_vars).unwrap());
    assert!(eval_bool("-1", &no_vars).unwrap());
}

#[test]
fn eval_bool_treats_zero_as_false() {
    assert!(!eval_bool("0", &no_vars).unwrap());
}

#[test]
fn eval_bool_evaluates_real_condition() {
    assert!(eval_bool("injLayout != 0 && nCylinders == 4", &lookup).unwrap());
}

#[test]
fn eval_bool_propagates_errors() {
    let err = eval_bool("unknownSymbol", &no_vars).unwrap_err();
    assert_eq!(err, ExprError::UnknownVar("unknownSymbol".to_string()));
}

// ---------------------------------------------------------------------
// 2.3 — unsupported function-call forms
// ---------------------------------------------------------------------

#[test]
fn bit_string_value_call_is_unsupported() {
    let err = eval("bitStringValue(label, sel)", &no_vars).unwrap_err();
    assert_eq!(err, ExprError::UnsupportedFn("bitStringValue".to_string()));
}

#[test]
fn table_call_with_string_literal_arg_is_unsupported() {
    // Real form: `table(x, "f.inc")` — the string literal must not force a
    // lexer error; the function-call form itself is what is unsupported.
    let err = eval(r#"table(x, "f.inc")"#, &no_vars).unwrap_err();
    assert_eq!(err, ExprError::UnsupportedFn("table".to_string()));
}

#[test]
fn unsupported_fn_inside_larger_expression_still_reported() {
    let err = eval("1 + bitStringValue(a, b)", &no_vars).unwrap_err();
    assert_eq!(err, ExprError::UnsupportedFn("bitStringValue".to_string()));
}

// ---------------------------------------------------------------------
// 2.4 — edge cases
// ---------------------------------------------------------------------

#[test]
fn division_by_zero_is_a_math_error() {
    let err = eval("1 / 0", &no_vars).unwrap_err();
    assert_eq!(err, ExprError::Math);
}

#[test]
fn division_by_zero_variable_is_a_math_error() {
    let err = eval("1 / zero", &lookup).unwrap_err();
    assert_eq!(err, ExprError::Math);
}

#[test]
fn empty_expression_is_an_error() {
    assert!(eval("", &no_vars).is_err());
}

#[test]
fn blank_expression_is_an_error() {
    assert!(eval("   ", &no_vars).is_err());
}

#[test]
fn deeply_nested_parens_are_bounded() {
    let depth = 200;
    let expr = format!("{}1{}", "(".repeat(depth), ")".repeat(depth));
    let err = eval(&expr, &no_vars).unwrap_err();
    assert_eq!(err, ExprError::TooDeep);
}

#[test]
fn moderately_nested_parens_still_work() {
    let depth = 10;
    let expr = format!("{}1{}", "(".repeat(depth), ")".repeat(depth));
    assert_eq!(eval(&expr, &no_vars).unwrap(), 1.0);
}

#[test]
fn trailing_garbage_after_expression_is_an_error() {
    assert!(eval("1 2", &no_vars).is_err());
}

#[test]
fn unbalanced_open_paren_is_an_error() {
    assert!(eval("(1 + 2", &no_vars).is_err());
}

#[test]
fn unbalanced_close_paren_is_an_error() {
    assert!(eval("1 + 2)", &no_vars).is_err());
}

#[test]
fn bare_equals_sign_is_a_syntax_error() {
    // `=` alone is not in the grammar (only `==`).
    assert!(eval("1 = 2", &no_vars).is_err());
}
