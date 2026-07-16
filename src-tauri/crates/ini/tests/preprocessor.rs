// SPDX-License-Identifier: GPL-3.0-or-later
//! Integration tests for the INI preprocessor — sub-step 1.1.
//!
//! **Written fresh** (no port): neither reference source covers a full
//! symbol-based preprocessor with nested `#if`/`#ifdef`/`#ifndef`/`#elif`.
//! `hyper-tuner/ini` only handles `#define`; the real Speeduino INI gates
//! constants behind `#if`/`#else`/`#endif` blocks referencing bare symbols
//! (e.g. `LAMBDA`, `mcu_stm32`, `COMMS_COMPAT`), never arithmetic
//! expressions, so symbol-only resolution (no expression evaluator) is
//! complete for this firmware family.
//!
//! Structural reference only: `adbancroft/TunerStudioIniParser`'s
//! `pre_processor.lark` grammar (LGPLv3) — consulted to confirm directive
//! set and nesting shape, not for implementation code. `#include` is
//! intentionally NOT implemented: real Speeduino INIs use none, and
//! honoring it would require a filesystem-aware API this crate does not
//! have. This is a known, documented limitation.

use opentune_ini::{parse_definition, preprocess};
use std::collections::HashSet;

fn symbols(names: &[&str]) -> HashSet<String> {
    names.iter().map(|s| s.to_string()).collect()
}

#[test]
fn selects_if_branch_when_symbol_is_active() {
    let ini = "#if LAMBDA\nwueAFR = lambda_variant\n#else\nwueAFR = afr_variant\n#endif\n";
    let out = preprocess(ini, &symbols(&["LAMBDA"]));
    assert!(out.contains("lambda_variant"));
    assert!(!out.contains("afr_variant"));
}

#[test]
fn selects_else_branch_when_symbol_is_inactive() {
    let ini = "#if LAMBDA\nwueAFR = lambda_variant\n#else\nwueAFR = afr_variant\n#endif\n";
    let out = preprocess(ini, &symbols(&[]));
    assert!(!out.contains("lambda_variant"));
    assert!(out.contains("afr_variant"));
}

#[test]
fn strips_directive_lines_from_output() {
    let ini = "#if LAMBDA\nkeep = 1\n#endif\n";
    let out = preprocess(ini, &symbols(&["LAMBDA"]));
    assert!(!out.contains("#if"));
    assert!(!out.contains("#endif"));
}

#[test]
fn handles_indented_directives() {
    // Real speeduino.ini has directives indented inside [Constants].
    let ini = "    #if MSDROID_COMPAT\n    a = 1\n    #else\n    b = 2\n    #endif\n";
    let out = preprocess(ini, &symbols(&[]));
    assert!(out.contains("b = 2"));
    assert!(!out.contains("a = 1"));
}

#[test]
fn ifdef_takes_branch_when_symbol_was_defined() {
    let ini = "#define FOO = 1\n#ifdef FOO\nyes\n#else\nno\n#endif\n";
    let out = preprocess(ini, &symbols(&[]));
    assert!(out.contains("yes"));
    assert!(!out.contains("no"));
}

#[test]
fn ifndef_takes_branch_when_symbol_was_not_defined() {
    let ini = "#ifndef FOO\nno_foo\n#else\nhas_foo\n#endif\n";
    let out = preprocess(ini, &symbols(&[]));
    assert!(out.contains("no_foo"));
    assert!(!out.contains("has_foo"));
}

#[test]
fn set_and_unset_mutate_the_active_symbol_set() {
    let ini = "#set FOO\n#if FOO\nyes\n#endif\n#unset FOO\n#if FOO\nno\n#endif\n";
    let out = preprocess(ini, &symbols(&[]));
    assert!(out.contains("yes"));
    assert!(!out.contains("no"));
}

#[test]
fn nested_if_resolves_independently_of_outer_branch() {
    let ini =
        "#if A\n  #if B\n  a_and_b\n  #else\n  a_and_not_b\n  #endif\n#else\n  not_a\n#endif\n";
    let out = preprocess(ini, &symbols(&["A"]));
    assert!(out.contains("a_and_not_b"));
    assert!(!out.contains("a_and_b"));
    assert!(!out.contains("not_a"));
}

#[test]
fn elif_selects_first_matching_branch() {
    let ini = "#if A\nfirst\n#elif B\nsecond\n#elif C\nthird\n#else\nfourth\n#endif\n";
    let out = preprocess(ini, &symbols(&["C"]));
    assert!(out.contains("third"));
    assert!(!out.contains("first"));
    assert!(!out.contains("second"));
    assert!(!out.contains("fourth"));
}

#[test]
fn passthrough_lines_outside_any_directive_are_kept_verbatim() {
    let ini = "[Constants]\nplain = 1\n";
    let out = preprocess(ini, &symbols(&[]));
    assert_eq!(out.trim(), "[Constants]\nplain = 1".trim());
}

fn parseable_ini_with(trailing: &str) -> String {
    format!(
        r#"[MegaTune]
signature = "test"
queryCommand = "Q"
versionInfo = "S"
ochGetCommand = "r"
pageReadCommand = "p"
pageValueWrite = "w"
burnCommand = "b"
blockingFactor = 1
blockReadTimeout = 1

[Constants]
pageSize = 1
page = 1

{trailing}
"#
    )
}

#[test]
fn unmatched_preprocessor_directives_produce_pointed_diagnostics() {
    let ini = parseable_ini_with("#endif\n#else\n");
    let def = parse_definition(&ini).expect("unmatched directives should degrade diagnostically");
    assert!(def.diagnostics.iter().any(|diagnostic| {
        diagnostic.section == "Preprocessor"
            && diagnostic.detail.contains("unmatched #endif")
            && diagnostic.detail.contains("line")
    }));
    assert!(def.diagnostics.iter().any(|diagnostic| {
        diagnostic.section == "Preprocessor" && diagnostic.detail.contains("unmatched #else")
    }));
}

#[test]
fn unclosed_preprocessor_block_produces_opener_diagnostic() {
    let ini = parseable_ini_with("#if NEVER_DEFINED\nhidden = 1\n");
    let def = parse_definition(&ini).expect("unclosed block should degrade diagnostically");
    assert!(def.diagnostics.iter().any(|diagnostic| {
        diagnostic.section == "Preprocessor"
            && diagnostic.detail.contains("unclosed #if")
            && diagnostic.detail.contains("missing #endif")
    }));
}

#[test]
fn expands_dollar_reference_to_define_value_list() {
    // rusEFI INIs define pin dictionaries once (`#define gpio_list="NONE",
    // "PA0", ...`) and reference them from bits option lists as `$gpio_list`.
    // The reference must expand to the define's value text.
    let ini = "#define pin_list=\"NONE\", \"PA0\", \"PA1\"\nmainRelayPin = bits, U16, 48, [0:8], $pin_list\n";
    let out = preprocess(ini, &symbols(&[]));
    assert!(
        out.contains("mainRelayPin = bits, U16, 48, [0:8], \"NONE\", \"PA0\", \"PA1\""),
        "got: {out}"
    );
    assert!(!out.contains("$pin_list"));
}

#[test]
fn chained_defines_expand_and_unknown_references_pass_through() {
    let ini = "#define base_list=\"A\", \"B\"\n#define full_list=$base_list, \"C\"\npin = bits, U08, 0, [0:1], $full_list\nnote = \"$100 or $nothing\"\n";
    let out = preprocess(ini, &symbols(&[]));
    assert!(
        out.contains("pin = bits, U08, 0, [0:1], \"A\", \"B\", \"C\""),
        "got: {out}"
    );
    // No matching define → the text is left exactly as written.
    assert!(out.contains("\"$100 or $nothing\""));
}
