// SPDX-License-Identifier: GPL-3.0-or-later
//! Parses the bundled simulator/demo INI (`resources/speeduino.sample.ini`)
//! end-to-end and pins two invariants the rest of the app relies on:
//! diagnostic-free parsing (nothing silently degraded) and a real
//! `[VeAnalyze]` binding (M4 Task 9 — the ground truth the auto-tune demo
//! flattens). This is the sample INI's own golden gate — separate from
//! `real_ini.rs`, which pins the real `speeduino.ini` and must stay
//! untouched by sample-INI changes.

use opentune_ini::parse_definition;

const SAMPLE_INI: &str = include_str!("../../../resources/speeduino.sample.ini");

#[test]
fn bundled_sample_ini_parses_diagnostic_free_with_a_ve_analyze_binding() {
    let def = parse_definition(SAMPLE_INI).expect("bundled sample INI must parse");
    assert!(
        def.diagnostics.is_empty(),
        "the shipped sample INI must parse without degradation, got {:?}",
        def.diagnostics
    );
    assert!(
        def.ve_analyze.is_some(),
        "M4 Task 9: the sample INI must declare a [VeAnalyze] binding"
    );
}
