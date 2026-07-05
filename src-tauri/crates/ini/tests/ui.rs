// SPDX-License-Identifier: GPL-3.0-or-later
//! Integration tests for `parse_definition`'s `[Menu]`/`[UserDefined]`/
//! `[TableEditor]`/`[CurveEditor]` parsing — sub-steps 3.2 and 3.3.
//!
//! Port source (ADR-0006): [`hyper-tuner/ini`](https://github.com/hyper-tuner/ini)
//! (MIT) — `parseMenu`, `parseDialogs`, `parseTables`, `parseCurves` cover all
//! four sections and establish the tolerated optional-trailing-token forms
//! exercised below (`subMenu` with page + `{cond}`; `field` with a `{}`
//! placeholder + trailing `{cond}` — hyper-tuner takes the *last* brace as
//! the condition and ignores an empty placeholder brace, which this port
//! follows).
//!
//! Extended beyond hyper-tuner (its own `// TODO: missing fields` in
//! `parseDialogs`) for `commandButton`, `slider`, `displayOnlyField`,
//! `settingSelector` — real `speeduino.ini` uses all four. Per the graceful
//! degradation contract: `commandButton` triggers an ECU command and has no
//! faithful frozen `FieldKind`, so it is dropped with a `Diagnostic` rather
//! than invented as a new field kind; `slider`/`displayOnlyField` reference
//! a bound constant and degrade faithfully to `FieldKind::Constant` instead
//! (exercised via the fixture's `slider` line below).
//!
//! Cross-reference diagnostics are exercised symmetrically for both
//! `[TableEditor]` and `[CurveEditor]` — a curve's `xBins`/`yBins` names a
//! constant exactly like a table's does.
//!
//! `tests/fixtures/speeduino-ui.ini` is trimmed from the real Speeduino
//! `speeduino.ini` (GPL-3, noisymime/speeduino) — see the fixture's own
//! header comment for full provenance and the exact construct set it
//! exercises.

use opentune_ini::{parse_definition, Diagnostic, FieldKind, Number};

fn fixture() -> &'static str {
    include_str!("fixtures/speeduino-ui.ini")
}

/// The minimal `[MegaTune]` block every inline fixture in this file needs to
/// satisfy `parse_comms`'s required fields — shared so each test's INI body
/// only has to supply the section(s) it actually exercises.
const COMMS_HEADER: &str = r#"
[MegaTune]
   signature            = "speeduino 202504-dev"
   queryCommand         = "Q"
   versionInfo          = "S"
   pageActivationDelay  = 10
   blockReadTimeout     = 2000
   interWriteDelay      = 10
   blockingFactor       = 251
   endianness           = little
   messageEnvelopeFormat = msEnvelope_1.0
   ochGetCommand        = "r"
   pageReadCommand      = "p%2i%2o%2c"
   pageValueWrite       = "M%2i%2o%2c%v"
   burnCommand          = "b%2i"
"#;

#[test]
fn parses_ui() {
    let def = parse_definition(fixture()).expect("fixture should parse");

    // ── menus ────────────────────────────────────────────────────────────
    assert_eq!(def.menus.len(), 1, "expected one top-level menu");
    let tuning = &def.menus[0];
    // "&Tuning" — the `&` mnemonic marker is stripped from the label.
    assert_eq!(tuning.label, "Tuning");

    // `subMenu = std_separator` has no representable target and is skipped;
    // only the two titled subMenus become items.
    assert_eq!(tuning.items.len(), 2);
    assert_eq!(tuning.items[0].label, "Engine Constants");
    assert_eq!(tuning.items[0].dialog, "engine_constants");

    // The full `subMenu = name, "Title", page, {cond}` form: page and
    // condition are tolerated (parsed without erroring) but dropped — a
    // `MenuItem` only carries label + dialog per the frozen shape.
    assert_eq!(tuning.items[1].label, "Sequential fuel trim (5-8)");
    assert_eq!(tuning.items[1].dialog, "inj_trimad_B");

    // ── dialogs ──────────────────────────────────────────────────────────
    assert_eq!(def.dialogs.len(), 1);
    let dialog = &def.dialogs[0];
    assert_eq!(dialog.name, "engine_constants_southwest");
    assert_eq!(dialog.title, "Speeduino Board");

    // Plain `field = "Label", constName` — always visible/enabled.
    let injector_layout = &dialog.fields[0];
    assert_eq!(
        injector_layout.kind,
        FieldKind::Constant("injLayout".to_string())
    );
    assert_eq!(injector_layout.visible, None);
    assert_eq!(injector_layout.enable, None);

    // `field = "Label", constName, { cond }` — the required raw visible-
    // condition assertion. Stored as a raw string, NOT evaluated.
    let pairing = &dialog.fields[1];
    assert_eq!(
        pairing.kind,
        FieldKind::Constant("inj4CylPairing".to_string())
    );
    assert_eq!(pairing.visible.as_deref(), Some("injLayout != 0"));
    assert_eq!(pairing.enable, None);

    // `field = "Label", constName, {}, { cond }` — the 4-arg placeholder
    // form. Tolerated: still produces a Constant field with the trailing
    // brace as `visible`; the empty `{}` placeholder is dropped.
    let aux = &dialog.fields[2];
    assert_eq!(aux.kind, FieldKind::Constant("inj4CylPairing".to_string()));
    assert_eq!(
        aux.visible.as_deref(),
        Some("injLayout != 0 && nFuelChannels == 4")
    );

    // `slider = "Label", constName, horizontal, { cond }` — references a
    // bound constant like a plain `field`; degrades faithfully to
    // `FieldKind::Constant` (the layout hint `horizontal` is dropped).
    let slider = &dialog.fields[3];
    assert_eq!(slider.kind, FieldKind::Constant("FILTER_FLEX".to_string()));
    assert_eq!(slider.visible.as_deref(), Some("injLayout != 0"));

    // `commandButton` triggers an ECU command — not representable by any
    // frozen `FieldKind` — so it must NOT appear as a field...
    assert_eq!(
        dialog.fields.len(),
        4,
        "commandButton must not produce a dialog field"
    );
    // ...and must instead be recorded as a Diagnostic.
    assert!(
        def.diagnostics.iter().any(|d| d.section == "UserDefined"
            && d.detail.contains("commandButton")
            && d.detail.contains("cmdVSSratio1")),
        "expected a Diagnostic for the unrepresentable commandButton field, got: {:?}",
        def.diagnostics
    );

    // ── tables ───────────────────────────────────────────────────────────
    assert_eq!(def.tables.len(), 1);
    let ve_table = &def.tables[0];
    assert_eq!(ve_table.name, "veTable1Tbl");
    assert_eq!(ve_table.x_bins, "rpmBins");
    assert_eq!(ve_table.y_bins, "fuelLoadBins");
    assert_eq!(ve_table.z, "veTable");

    // ── curves ───────────────────────────────────────────────────────────
    assert_eq!(def.curves.len(), 1);
    let curve = &def.curves[0];
    assert_eq!(curve.name, "time_accel_tpsdot_curve");
    assert_eq!(curve.x_bins, "taeBins");
    assert_eq!(curve.y_bins, "taeRates");

    // Every name the table/curve bins reference (rpmBins, fuelLoadBins,
    // veTable, taeBins, taeRates) is defined in `[Constants]` above, so the
    // happy path must produce zero cross-reference diagnostics — locks in
    // that the fixture's `[Constants]` block stays complete over time.
    assert!(
        def.diagnostics
            .iter()
            .all(|d| d.section != "TableEditor" && d.section != "CurveEditor"),
        "expected no table/curve cross-reference diagnostics on the happy path, got: {:?}",
        def.diagnostics
    );
}

/// A table OR a curve referencing a constant name that doesn't exist in
/// `[Constants]` must record a `Diagnostic`, not panic and not fail the
/// whole parse — symmetrically for both section kinds (a curve's
/// `xBins`/`yBins` names a constant exactly like a table's does). Kept as a
/// separate minimal inline INI (not the golden fixture) so the golden
/// fixture's happy path stays diagnostic-free.
#[test]
fn table_and_curve_referencing_missing_constant_records_diagnostic_not_panic() {
    let ini = format!(
        r#"{COMMS_HEADER}
[Constants]
    pageSize = 4
page = 1
      veTable = array, U08, 0, [2x2], "%", 1.0, 0.0, 0.0, 255.0, 0

[TableEditor]
   table = veTable1Tbl, veTable1Map, "VE Table", 2
      xBins = missingXBins, rpm
      yBins = missingYBins, fuelLoad
      zBins = veTable

[CurveEditor]
   curve = time_accel_tpsdot_curve, "TPS based AE"
      xBins = missingCurveXBins, TPSdot
      yBins = missingCurveYBins
"#
    );

    let def = parse_definition(&ini).expect("parse must not fail on a missing cross-reference");

    assert_eq!(def.tables.len(), 1);
    assert_eq!(def.tables[0].x_bins, "missingXBins");
    assert_eq!(def.tables[0].y_bins, "missingYBins");
    assert_eq!(def.curves.len(), 1);
    assert_eq!(def.curves[0].x_bins, "missingCurveXBins");
    assert_eq!(def.curves[0].y_bins, "missingCurveYBins");

    let table_diagnostics: Vec<&Diagnostic> = def
        .diagnostics
        .iter()
        .filter(|d| d.section == "TableEditor")
        .collect();
    assert!(
        table_diagnostics
            .iter()
            .any(|d| d.detail.contains("missingXBins")),
        "expected a Diagnostic naming the missing table x_bins constant, got: {:?}",
        def.diagnostics
    );
    assert!(
        table_diagnostics
            .iter()
            .any(|d| d.detail.contains("missingYBins")),
        "expected a Diagnostic naming the missing table y_bins constant, got: {:?}",
        def.diagnostics
    );

    let curve_diagnostics: Vec<&Diagnostic> = def
        .diagnostics
        .iter()
        .filter(|d| d.section == "CurveEditor")
        .collect();
    assert!(
        curve_diagnostics
            .iter()
            .any(|d| d.detail.contains("missingCurveXBins")),
        "expected a Diagnostic naming the missing curve x_bins constant, got: {:?}",
        def.diagnostics
    );
    assert!(
        curve_diagnostics
            .iter()
            .any(|d| d.detail.contains("missingCurveYBins")),
        "expected a Diagnostic naming the missing curve y_bins constant, got: {:?}",
        def.diagnostics
    );
}

/// A genuinely-unknown dialog keyword inside `[UserDefined]` (not one of the
/// recognised `dialog`/`panel`/`field`/`commandButton`/`settingSelector`/
/// `slider`/`displayOnlyField`/`topicHelp` keywords) must still let the parse
/// succeed, must NOT produce a dialog field, and must be recorded as a
/// `Diagnostic` naming the unrecognised keyword — the graceful-degradation
/// contract documented on `ui_dialog_parser.rs`, not a silent drop.
#[test]
fn unknown_dialog_keyword_records_diagnostic_not_dropped_silently() {
    let ini = format!(
        r#"{COMMS_HEADER}
[Constants]
    pageSize = 4
page = 1

[UserDefined]
   dialog = engine_constants_southwest, "Speeduino Board"
      frobnicator = "X", someVar
"#
    );

    let def = parse_definition(&ini).expect("parse must not fail on an unknown dialog keyword");

    assert_eq!(def.dialogs.len(), 1);
    let dialog = &def.dialogs[0];
    assert_eq!(
        dialog.fields.len(),
        0,
        "unknown dialog keyword must not produce a dialog field"
    );

    assert!(
        def.diagnostics
            .iter()
            .any(|d| d.section == "UserDefined" && d.detail.contains("frobnicator")),
        "expected a Diagnostic naming the unknown dialog keyword `frobnicator`, got: {:?}",
        def.diagnostics
    );
}

/// M4 Task 2: the FULL `[TableEditor]`/`[CurveEditor]` attribute set — real
/// grammar verbatim from `speeduino.ini` @ 0832dc1d (l.4935-4948 table header
/// + l.4621-4630 curve), not just the M2-era `xBins`/`yBins`/`zBins` subset.
#[test]
fn parses_full_table_and_curve_attributes() {
    let ini = format!(
        r#"{COMMS_HEADER}
[Constants]
    pageSize = 288, 288

page = 2
      veTable    = array,  U08,   0, [16x16], "%",   1.0, 0.0, 0.0, 255.0, 0
      rpmBins    = array,  U08, 256, [16],  "RPM", 100.0, 0.0, 100.0, 25500.0, 0
      fuelLoadBins = array, U08, 272, [16], "kPa", 1.0, 0.0, 0.0, 511.0, 0
page = 1
      taeBins    = array,  U08,   0, [4], "%/s", 10.0, 0.0, 0.0, 2550.0, 0
      taeRates   = array,  U08,   4, [4], "%",    1.0, 0.0, 0.0, 255.0, 0

[TableEditor]
   table = veTable1Tbl,  veTable1Map,  "VE Table",   2
      topicHelp   = "http://wiki.speeduino.com/en/configuration/VE_table"
      xBins       = rpmBins,  rpm
      yBins       = fuelLoadBins, fuelLoad
      xyLabels    = "RPM", "Fuel Load: "
      zBins       = veTable
      gridHeight  = 2.0
      gridOrient  = 250,   0, 340
      upDownLabel = "(RICHER)", "(LEANER)"

[CurveEditor]
      curve = time_accel_tpsdot_curve, "TPS based AE"
            columnLabel = "TPSdot", "Added"
            xAxis = 0, 1200, 6
            yAxis = 0, 250, 4
            xBins = taeBins, TPSdot
            yBins = taeRates
            size  = 400, 400
"#
    );
    let def = parse_definition(&ini).expect("parses");
    let t = def.table("veTable1Tbl").expect("table by id");
    assert_eq!(t.map3d_id, "veTable1Map");
    assert_eq!(t.title, "VE Table");
    assert_eq!(t.page, 2);
    assert_eq!(
        (t.x_bins.as_str(), t.x_channel.as_str()),
        ("rpmBins", "rpm")
    );
    assert_eq!(
        (t.y_bins.as_str(), t.y_channel.as_str()),
        ("fuelLoadBins", "fuelLoad")
    );
    assert_eq!(t.z, "veTable");
    assert_eq!(t.xy_labels, vec!["RPM", "Fuel Load: "]);
    assert!((t.grid_height - 2.0).abs() < 1e-9);
    assert_eq!(t.grid_orient, vec![250.0, 0.0, 340.0]);
    assert_eq!(t.up_down_label, vec!["(RICHER)", "(LEANER)"]);
    assert_eq!(
        t.help,
        "http://wiki.speeduino.com/en/configuration/VE_table"
    );
    let c = def.curve("time_accel_tpsdot_curve").expect("curve by id");
    assert_eq!(c.title, "TPS based AE");
    assert_eq!(c.column_labels, vec!["TPSdot", "Added"]);
    let x = c.x_axis.as_ref().expect("xAxis");
    assert_eq!(
        (x.min.clone(), x.max.clone(), x.divisions),
        (Number::Lit(0.0), Number::Lit(1200.0), 6)
    );
    assert_eq!(
        (c.x_bins.as_str(), c.x_channel.as_str()),
        ("taeBins", "TPSdot")
    );
    assert_eq!(c.y_bins, "taeRates");
    assert_eq!(c.size, vec![400.0, 400.0]);
}
