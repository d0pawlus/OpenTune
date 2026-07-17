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
fn unary_plus_preserves_literals_and_nested_values() {
    assert_eq!(eval("+5", &no_vars).unwrap(), 5.0);
    assert_eq!(eval("2 * +(1 + 2)", &no_vars).unwrap(), 6.0);
    assert_eq!(eval("2 + +3", &no_vars).unwrap(), 5.0);
}

#[test]
fn exponent_notation_is_accepted() {
    assert_eq!(eval("1e3", &no_vars).unwrap(), 1_000.0);
    assert_eq!(eval("2.5E-2", &no_vars).unwrap(), 0.025);
    assert_eq!(eval(".5e+2", &no_vars).unwrap(), 50.0);
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
fn logical_and_false_when_rhs_falsy() {
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
//
// Contract updated with function-call support (MS3): arguments are now
// full expressions whose own errors propagate precisely; a call whose
// args resolve but that no resolver knows still reports UnsupportedFn.
// Either way the exotic real-INI forms degrade to a clean error.
// ---------------------------------------------------------------------

#[test]
fn bit_string_value_call_is_unsupported() {
    // Args resolve (plain variables) — the CALL is what plain eval refuses.
    let err = eval("bitStringValue(injLayout, nCylinders)", &lookup).unwrap_err();
    assert_eq!(err, ExprError::UnsupportedFn("bitStringValue".to_string()));
}

#[test]
fn table_call_with_string_literal_arg_is_a_clean_error() {
    // Real form: `table(x, "f.inc")` — the grammar deliberately does not
    // lex string literals, so the argument list fails as a syntax error
    // (clean, never a panic or silent misparse).
    let err = eval(r#"table(injLayout, "f.inc")"#, &lookup).unwrap_err();
    assert!(matches!(err, ExprError::Syntax(_)), "got {err:?}");
}

#[test]
fn unsupported_fn_inside_larger_expression_still_reported() {
    let err = eval("1 + bitStringValue(injLayout, nCylinders)", &lookup).unwrap_err();
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

// ---------------------------------------------------------------------
// 3.4/3.5 (M3 Task 3) — bitwise `&` and `<<`
//
// Real indicators/computed channels use them:
// `syncStatus = { halfSync + (sync << 1) }`, `{ sd_status & 1 }`.
// Precedence is standard C: shift binds tighter than comparison; `&` sits
// between equality and `&&`. Values are f64 cast to i64 and back.
// ---------------------------------------------------------------------

/// Lookup for the real-INI bitwise expressions above.
fn bit_lookup(name: &str) -> Option<f64> {
    match name {
        "sync" => Some(1.0),
        "halfSync" => Some(1.0),
        "sd_status" => Some(3.0),
        _ => None,
    }
}

#[test]
fn eval_supports_bitwise_and_and_shift() {
    let lookup = |n: &str| bit_lookup(n);
    assert_eq!(eval("halfSync + (sync << 1)", &lookup).unwrap(), 3.0);
    assert_eq!(eval("sd_status & 1", &lookup).unwrap(), 1.0);
}

#[test]
fn shift_left_is_left_associative() {
    // Right-associative evaluation would give `1 << (1 << 2)` = 16.
    assert_eq!(eval("1 << 1 << 2", &no_vars).unwrap(), 8.0);
}

#[test]
fn addition_binds_tighter_than_shift() {
    // C precedence: `(1 + 1) << 2` = 8. If `<<` bound tighter: `1 + (1 << 2)` = 5.
    assert_eq!(eval("1 + 1 << 2", &no_vars).unwrap(), 8.0);
}

#[test]
fn shift_binds_tighter_than_comparison() {
    // C precedence: `(1 << 2) < 8` = 1. If `<` bound tighter: `1 << (2 < 8)` = 2.
    assert_eq!(eval("1 << 2 < 8", &no_vars).unwrap(), 1.0);
    // C precedence: `2 == (1 << 1)` = 1. If `==` bound tighter: `(2 == 1) << 1` = 0.
    assert_eq!(eval("2 == 1 << 1", &no_vars).unwrap(), 1.0);
}

#[test]
fn shift_lexes_distinctly_from_less_and_less_equal() {
    // `<` and `<=` must still tokenize when `<<` exists in the grammar.
    assert_eq!(eval("1 < 2", &no_vars).unwrap(), 1.0);
    assert_eq!(eval("2 <= 2", &no_vars).unwrap(), 1.0);
}

#[test]
fn bitand_binds_looser_than_equality() {
    // C precedence: `2 & (2 == 2)` = `2 & 1` = 0.
    // If `&` bound tighter than `==`: `(2 & 2) == 2` = 1.
    assert_eq!(eval("2 & 2 == 2", &no_vars).unwrap(), 0.0);
}

#[test]
fn bitand_binds_tighter_than_logical_and() {
    // C precedence: `2 & (2 && 4)`? No — `&` is TIGHTER: `(2 & 2) && 4` = 1.
    // If `&` bound looser than `&&`: `2 & (2 && 4)` = `2 & 1` = 0.
    assert_eq!(eval("2 & 2 && 4", &no_vars).unwrap(), 1.0);
}

#[test]
fn single_ampersand_does_not_consume_logical_and() {
    // `&&` must keep its logical meaning (`2 && 4` = 1), not degrade into
    // bitwise `2 & (&4)` (a syntax error) or `2 & 4` (= 0).
    assert_eq!(eval("2 && 4", &no_vars).unwrap(), 1.0);
}

#[test]
fn bitwise_ops_truncate_fractional_operands() {
    // f64 -> i64 casts truncate toward zero: `2.9 & 3` = `2 & 3` = 2.
    assert_eq!(eval("2.9 & 3", &no_vars).unwrap(), 2.0);
}

#[test]
fn bitand_evaluates_eagerly_like_other_operators() {
    // Same eager-evaluation contract as `&&`/`||`: an unknown variable on
    // the right of `&` surfaces even though the left side is 0.
    let err = eval("0 & bogusVar", &no_vars).unwrap_err();
    assert_eq!(err, ExprError::UnknownVar("bogusVar".to_string()));
}

#[test]
fn out_of_range_shift_count_is_a_math_error() {
    // i64 shifts are only defined for counts 0..=63; degrade loudly like
    // division by zero instead of panicking or wrapping silently.
    assert_eq!(eval("1 << 64", &no_vars).unwrap_err(), ExprError::Math);
    assert_eq!(eval("1 << -1", &no_vars).unwrap_err(), ExprError::Math);
}

#[test]
fn eval_bool_supports_indicator_style_bitwise_expr() {
    let lookup = |n: &str| bit_lookup(n);
    assert!(eval_bool("sd_status & 1", &lookup).unwrap());
    assert!(!eval_bool("sd_status & 4", &lookup).unwrap());
}

// ---------------------------------------------------------------------
// Ternary conditional — MS3-era INIs compute bounds and even scale
// factors with it (`{ clt_exp ? 230 : 120 }`,
// `{ prefSpeedUnits == 0 ? 0.22369 : 0.36 }`).
// ---------------------------------------------------------------------

#[test]
fn evaluates_ternary_conditional() {
    assert_eq!(eval("1 ? 2 : 3", &no_vars).unwrap(), 2.0);
    assert_eq!(eval("0 ? 2 : 3", &no_vars).unwrap(), 3.0);
}

#[test]
fn ternary_binds_below_comparison_like_c() {
    // The whole comparison is the condition, per C precedence.
    let lookup = |n: &str| match n {
        "prefSpeedUnits" => Some(0.0),
        _ => None,
    };
    assert_eq!(
        eval("prefSpeedUnits == 0 ?  0.22369 :  0.36", &lookup).unwrap(),
        0.22369
    );
}

#[test]
fn ternary_nests_right_associatively() {
    assert_eq!(eval("0 ? 1 : 0 ? 2 : 3", &no_vars).unwrap(), 3.0);
    assert_eq!(eval("1 ? 0 ? 4 : 5 : 6", &no_vars).unwrap(), 5.0);
}

#[test]
fn ternary_with_variable_condition_matches_ms3_clthighlim() {
    let lookup = |n: &str| match n {
        "clt_exp" => Some(0.0),
        _ => None,
    };
    assert_eq!(eval("clt_exp ? 230 : 120", &lookup).unwrap(), 120.0);
}

#[test]
fn ternary_missing_colon_is_a_syntax_error() {
    assert!(eval("1 ? 2", &no_vars).is_err());
}

// (Ternary branch resolution is LAZY, unlike `&&`/`||` — see the
// "PR #22 review findings" section at the end of this file.)

// ---------------------------------------------------------------------
// Function calls — `eval_with_functions` resolves TunerStudio builtins
// via a caller-supplied hook (MS3: `getChannelScaleByOffset(...)`).
// Plain `eval` keeps rejecting every call as UnsupportedFn.
// ---------------------------------------------------------------------

fn fake_funcs(name: &str, args: &[f64]) -> Option<f64> {
    match (name, args) {
        ("double", [x]) => Some(x * 2.0),
        ("add", [a, b]) => Some(a + b),
        _ => None,
    }
}

#[test]
fn eval_with_functions_dispatches_calls() {
    use opentune_ini::eval_with_functions;
    assert_eq!(
        eval_with_functions("double(21)", &no_vars, &fake_funcs).unwrap(),
        42.0
    );
    assert_eq!(
        eval_with_functions("add(1, 2) * 2", &no_vars, &fake_funcs).unwrap(),
        6.0
    );
}

#[test]
fn function_arguments_are_full_expressions_including_variables() {
    use opentune_ini::eval_with_functions;
    assert_eq!(
        eval_with_functions("double(nCylinders + 1)", &lookup, &fake_funcs).unwrap(),
        10.0
    );
}

#[test]
fn unknown_function_is_still_unsupported() {
    use opentune_ini::eval_with_functions;
    assert_eq!(
        eval_with_functions("mystery(1)", &no_vars, &fake_funcs).unwrap_err(),
        ExprError::UnsupportedFn("mystery".to_string())
    );
}

#[test]
fn plain_eval_still_rejects_all_function_calls() {
    assert_eq!(
        eval("double(21)", &no_vars).unwrap_err(),
        ExprError::UnsupportedFn("double".to_string())
    );
}

#[test]
fn function_call_inside_ternary_matches_ms3_pwm_scale_shape() {
    use opentune_ini::eval_with_functions;
    let vars = |n: &str| match n {
        "pwm_opt_curve_a" => Some(1.0),
        "pwm_opt_load_a_offset" => Some(18.0),
        _ => None,
    };
    let funcs = |name: &str, args: &[f64]| match (name, args) {
        ("getChannelScaleByOffset", [x]) if *x == 18.0 => Some(0.1),
        _ => None,
    };
    assert_eq!(
        eval_with_functions(
            "pwm_opt_curve_a == 0 ? 1 : getChannelScaleByOffset(pwm_opt_load_a_offset)",
            &vars,
            &funcs,
        )
        .unwrap(),
        0.1
    );
}

// ---------------------------------------------------------------------
// PR #22 review findings — ternary depth guard + lazy branch evaluation
// ---------------------------------------------------------------------

#[test]
fn deeply_nested_ternary_is_too_deep_not_a_stack_overflow() {
    // Review finding: ternary branches recursed without the MAX_DEPTH
    // guard, so a corrupt INI could overflow the stack. A nesting bomb
    // must degrade to TooDeep like pathological parentheses do.
    let bomb = format!("{}1{}", "1?".repeat(20_000), ":1".repeat(20_000));
    assert_eq!(eval(&bomb, &no_vars).unwrap_err(), ExprError::TooDeep);
}

#[test]
fn ternary_takes_only_the_selected_branch_like_tunerstudio() {
    // Review finding: eager `?:` diverged from TunerStudio — a tune with
    // an unconfigured PWM slot (`curve == 0 ? 1 : getChannelScaleByOffset(
    // badOffset)`) must resolve to 1, not fail the whole constant.
    assert_eq!(eval("1 ? 2 : bogusVar", &no_vars).unwrap(), 2.0);
    assert_eq!(eval("0 ? bogusVar : 3", &no_vars).unwrap(), 3.0);
    assert_eq!(eval("0 ? mystery(1) : 3", &no_vars).unwrap(), 3.0);
}

#[test]
fn ternary_condition_errors_still_propagate() {
    let err = eval("bogusVar ? 1 : 2", &no_vars).unwrap_err();
    assert_eq!(err, ExprError::UnknownVar("bogusVar".to_string()));
}

#[test]
fn taken_ternary_branch_errors_still_propagate() {
    let err = eval("1 ? bogusVar : 2", &no_vars).unwrap_err();
    assert_eq!(err, ExprError::UnknownVar("bogusVar".to_string()));
}

#[test]
fn untaken_branch_syntax_errors_still_surface() {
    // Laziness mutes RESOLUTION (names, functions), not grammar — the
    // untaken branch must still lex, or the parser cannot find its end.
    assert!(eval("1 ? 2 : )", &no_vars).is_err());
}
