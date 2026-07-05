// SPDX-License-Identifier: GPL-3.0-or-later
//! Integration tests for `[GaugeConfigurations]` + `[FrontPage]` parsing —
//! Task 3 (M3), sub-steps 3.1–3.3.
//!
//! **Written fresh** (ADR-0006 recorded exception): hyper-tuner/ini's
//! section switch ends at `[Datalog]` (`src/ini.ts` ~lines 146-190) — it
//! parses neither `[GaugeConfigurations]` nor `[FrontPage]`, so there is
//! nothing to port. The grammar truth-source is the real `speeduino.ini`
//! (noisymime/speeduino @ 63fd68e9, GPL-3 — consulted as a reference corpus
//! only, no code involved; the trimmed fixture records the exact lines
//! exercised):
//!
//! ```text
//! [GaugeConfigurations]
//! gaugeCategory = "Main"
//! name = channel, "title", "units", lo, hi, loD, loW, hiW, hiD, vd, ld
//!
//! [FrontPage]
//! gaugeN    = gaugeName
//! indicator = { expr }, "off label", "on label", offBg, offFg, onBg, onFg
//! ```
//!
//! Degradation contract (mirrors M2's table/curve cross-ref checks):
//! malformed rows and unknown constructs record a `Diagnostic` and are
//! skipped; a gauge referencing an unknown output channel (or a front-page
//! slot referencing an unknown gauge) records a `Diagnostic` but is kept.

use opentune_ini::{parse_definition, Number};

fn fixture() -> &'static str {
    include_str!("fixtures/speeduino-gauges.ini")
}

// ---------------------------------------------------------------------
// 3.1 — happy path: gauges, categories, front-page slots + indicators
// ---------------------------------------------------------------------

#[test]
fn parses_gauges_and_frontpage() {
    let ini = fixture();
    let def = parse_definition(ini).expect("parses");
    let tach = def.gauges.iter().find(|g| g.name == "tachometer").unwrap();
    assert_eq!(tach.channel, "rpm");
    assert_eq!(tach.title, "Engine Speed");
    assert_eq!(tach.category, "Main");
    assert_eq!(tach.high, Number::Lit(8000.0));
    assert_eq!(tach.hi_danger, Number::Lit(7500.0));
    assert_eq!(def.frontpage.gauge_slots, vec!["tachometer", "cltGauge"]);
    let ind = &def.frontpage.indicators[0];
    assert_eq!(ind.expr.trim(), "running");
    assert_eq!(ind.on_label, "Running");
    assert_eq!(ind.on_bg, "green");
}

#[test]
fn gauge_carries_all_positional_fields() {
    let def = parse_definition(fixture()).expect("parses");

    let tach = def.gauges.iter().find(|g| g.name == "tachometer").unwrap();
    assert_eq!(tach.units, "RPM");
    assert_eq!(tach.low, Number::Lit(0.0));
    assert_eq!(tach.lo_danger, Number::Lit(300.0));
    assert_eq!(tach.lo_warn, Number::Lit(600.0));
    assert_eq!(tach.hi_warn, Number::Lit(7000.0));
    assert_eq!(tach.value_digits, 0);
    assert_eq!(tach.label_digits, 0);

    // negative literal bounds + second category
    let clt = def.gauges.iter().find(|g| g.name == "cltGauge").unwrap();
    assert_eq!(clt.channel, "coolant");
    assert_eq!(clt.units, "C");
    assert_eq!(clt.low, Number::Lit(-40.0));
    assert_eq!(clt.lo_danger, Number::Lit(-15.0));
    assert_eq!(clt.category, "Main");

    let expr_gauge = def.gauges.iter().find(|g| g.name == "exprGauge").unwrap();
    assert_eq!(expr_gauge.category, "Sensors");
    assert_eq!(expr_gauge.value_digits, 1);
}

#[test]
fn indicator_fields_map_positionally() {
    let def = parse_definition(fixture()).expect("parses");
    assert_eq!(def.frontpage.indicators.len(), 2);

    let running = &def.frontpage.indicators[0];
    assert_eq!(running.expr, "running");
    assert_eq!(running.off_label, "Not Running");
    assert_eq!(running.on_label, "Running");
    assert_eq!(running.off_bg, "white");
    assert_eq!(running.off_fg, "black");
    assert_eq!(running.on_bg, "green");
    assert_eq!(running.on_fg, "black");

    // second indicator discriminates bg/fg ordering (on_bg=black, on_fg=white)
    let crank = &def.frontpage.indicators[1];
    assert_eq!(crank.expr, "crank");
    assert_eq!(crank.off_label, "Not Cranking");
    assert_eq!(crank.on_bg, "black");
    assert_eq!(crank.on_fg, "white");
}

// ---------------------------------------------------------------------
// 3.3 — robustness: `{ expr }` bounds, malformed rows, cross-ref checks
// ---------------------------------------------------------------------

#[test]
fn expression_bound_parses_to_number_expr_not_hard_error() {
    let def = parse_definition(fixture()).expect("parses");
    let g = def.gauges.iter().find(|g| g.name == "exprGauge").unwrap();
    assert_eq!(g.high, Number::Expr("rpmhigh".to_string()));
    // the other bounds of the same row stay literal
    assert_eq!(g.low, Number::Lit(0.0));
    assert_eq!(g.hi_danger, Number::Lit(7500.0));
}

#[test]
fn malformed_gauge_row_records_diagnostic_and_is_skipped() {
    let def = parse_definition(fixture()).expect("parses");
    assert!(
        !def.gauges.iter().any(|g| g.name == "brokenGauge"),
        "malformed row must be skipped, not half-parsed"
    );
    assert!(
        def.diagnostics
            .iter()
            .any(|d| d.section == "GaugeConfigurations" && d.detail.contains("brokenGauge")),
        "expected a GaugeConfigurations diagnostic for brokenGauge, got: {:?}",
        def.diagnostics
    );
}

#[test]
fn gauge_referencing_unknown_channel_records_diagnostic_but_is_kept() {
    let def = parse_definition(fixture()).expect("parses");
    // kept (degraded), like M2's table cross-ref check
    let ghost = def.gauges.iter().find(|g| g.name == "ghostGauge").unwrap();
    assert_eq!(ghost.channel, "nosuchvar");
    assert!(
        def.diagnostics.iter().any(|d| {
            d.section == "GaugeConfigurations"
                && d.detail.contains("ghostGauge")
                && d.detail.contains("nosuchvar")
        }),
        "expected a cross-ref diagnostic for ghostGauge -> nosuchvar, got: {:?}",
        def.diagnostics
    );
}

#[test]
fn happy_path_gauges_produce_no_cross_ref_diagnostics() {
    // Locks the cross-ref check's polarity: known channels must NOT be
    // flagged (only ghostGauge/brokenGauge may appear in diagnostics).
    let def = parse_definition(fixture()).expect("parses");
    let gauge_diags: Vec<_> = def
        .diagnostics
        .iter()
        .filter(|d| {
            (d.section == "GaugeConfigurations" || d.section == "FrontPage")
                && !d.detail.contains("ghostGauge")
                && !d.detail.contains("brokenGauge")
        })
        .collect();
    assert!(
        gauge_diags.is_empty(),
        "expected no diagnostics for well-formed gauges/slots, got: {gauge_diags:?}"
    );
}

// ---------------------------------------------------------------------
// 3.3 — front-page slot ordering + degradation (synthetic INI)
// ---------------------------------------------------------------------

/// Minimal comms header so `parse_definition` succeeds on synthetic INIs.
fn with_comms(body: &str) -> String {
    format!("{}\n{body}", include_str!("fixtures/minimal_comms.ini"))
}

#[test]
fn frontpage_slots_fill_in_numeric_not_lexicographic_order() {
    // gauge10 must sort AFTER gauge9 (lexicographic order would reverse
    // them), and declaration order must not matter.
    let ini = with_comms(
        "[FrontPage]\n\
         gauge2 = b\n\
         gauge10 = j\n\
         gauge9 = i\n\
         gauge1 = a\n",
    );
    let def = parse_definition(&ini).expect("parses");
    assert_eq!(def.frontpage.gauge_slots, vec!["a", "b", "i", "j"]);
}

#[test]
fn frontpage_slot_referencing_unknown_gauge_records_diagnostic_but_is_kept() {
    let ini = with_comms("[FrontPage]\ngauge1 = missingGauge\n");
    let def = parse_definition(&ini).expect("parses");
    assert_eq!(def.frontpage.gauge_slots, vec!["missingGauge"]);
    assert!(
        def.diagnostics
            .iter()
            .any(|d| d.section == "FrontPage" && d.detail.contains("missingGauge")),
        "expected a FrontPage cross-ref diagnostic, got: {:?}",
        def.diagnostics
    );
}

#[test]
fn indicator_with_missing_colors_degrades_to_empty_strings() {
    let ini = with_comms("[FrontPage]\nindicator = { running }, \"Off\", \"On\"\n");
    let def = parse_definition(&ini).expect("parses");
    let ind = &def.frontpage.indicators[0];
    assert_eq!(ind.expr, "running");
    assert_eq!(ind.off_label, "Off");
    assert_eq!(ind.on_label, "On");
    assert_eq!(ind.off_bg, "");
    assert_eq!(ind.on_fg, "");
}

#[test]
fn unknown_frontpage_construct_records_diagnostic_and_continues() {
    let ini = with_comms(
        "[FrontPage]\n\
         wizardry = 42\n\
         gauge1 = a\n",
    );
    let def = parse_definition(&ini).expect("parses");
    // the unknown key degrades; the section keeps parsing afterwards
    assert_eq!(def.frontpage.gauge_slots, vec!["a"]);
    assert!(
        def.diagnostics
            .iter()
            .any(|d| d.section == "FrontPage" && d.detail.contains("wizardry")),
        "expected a FrontPage diagnostic for `wizardry`, got: {:?}",
        def.diagnostics
    );
}

#[test]
fn malformed_indicator_records_diagnostic_and_is_skipped() {
    // no `{ expr }` head — unrecognisable as an indicator
    let ini = with_comms("[FrontPage]\nindicator = running, \"Off\", \"On\"\n");
    let def = parse_definition(&ini).expect("parses");
    assert!(def.frontpage.indicators.is_empty());
    assert!(
        def.diagnostics
            .iter()
            .any(|d| d.section == "FrontPage" && d.detail.contains("indicator")),
        "expected a FrontPage diagnostic for the malformed indicator, got: {:?}",
        def.diagnostics
    );
}

#[test]
fn ini_without_gauge_sections_yields_empty_defaults() {
    let ini = with_comms("");
    let def = parse_definition(&ini).expect("parses");
    assert!(def.gauges.is_empty());
    assert!(def.frontpage.gauge_slots.is_empty());
    assert!(def.frontpage.indicators.is_empty());
}
