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

use opentune_ini::{parse_definition, Diagnostic, FieldKind};

fn fixture() -> &'static str {
    include_str!("fixtures/speeduino-ui.ini")
}

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

    // `field = "Label", constName, { cond }` — the third position is the
    // enable condition. Stored as a raw string, NOT evaluated.
    let pairing = &dialog.fields[1];
    assert_eq!(
        pairing.kind,
        FieldKind::Constant("inj4CylPairing".to_string())
    );
    assert_eq!(pairing.visible, None);
    assert_eq!(pairing.enable.as_deref(), Some("injLayout != 0"));

    // `field = "Label", constName, {}, { cond }` — the 4-arg placeholder
    // form. The empty enable placeholder is preserved positionally and the
    // fourth token becomes `visible`.
    let aux = &dialog.fields[2];
    assert_eq!(aux.kind, FieldKind::Constant("inj4CylPairing".to_string()));
    assert_eq!(
        aux.visible.as_deref(),
        Some("injLayout != 0 && nFuelChannels == 4")
    );

    // `slider = "Label", constName, horizontal, { cond }` — references a
    // bound constant like a plain `field`; its condition follows the
    // orientation token and is therefore `enable`.
    let slider = &dialog.fields[3];
    assert_eq!(slider.kind, FieldKind::Constant("FILTER_FLEX".to_string()));
    assert_eq!(slider.visible, None);
    assert_eq!(slider.enable.as_deref(), Some("injLayout != 0"));

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
    let ini = r#"
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
"#;

    let def = parse_definition(ini).expect("parse must not fail on a missing cross-reference");

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
    let ini = r#"
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

[Constants]
    pageSize = 4
page = 1

[UserDefined]
   dialog = engine_constants_southwest, "Speeduino Board"
      frobnicator = "X", someVar
"#;

    let def = parse_definition(ini).expect("parse must not fail on an unknown dialog keyword");

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

#[test]
fn field_enable_and_visible_conditions_are_positioned_not_guessed() {
    let ini = format!(
        "{}\n\
[UserDefined]\n\
dialog = condition_positions, \"Conditions\"\n\
field = \"Enable only\", injLayout, {{ injLayout != 0 }}\n\
field = \"Visible only\", injLayout, {{}}, {{ nCylinders == 4 }}\n\
field = \"Both\", injLayout, {{ injLayout != 0 }}, {{ nCylinders == 4 }}\n\
displayOnlyField = \"Display\", injLayout, {{ enabledDisplay }}, {{ visibleDisplay }}\n\
slider = \"Slider\", injLayout, horizontal, {{ enabledSlider }}, {{ visibleSlider }}\n",
        fixture()
    );
    let def = parse_definition(&ini).expect("condition-position fixture should parse");
    let dialog = def
        .dialogs
        .iter()
        .find(|dialog| dialog.name == "condition_positions")
        .expect("condition-position dialog");

    assert_eq!(
        (
            dialog.fields[0].enable.as_deref(),
            dialog.fields[0].visible.as_deref()
        ),
        (Some("injLayout != 0"), None)
    );
    assert_eq!(
        (
            dialog.fields[1].enable.as_deref(),
            dialog.fields[1].visible.as_deref()
        ),
        (None, Some("nCylinders == 4"))
    );
    assert_eq!(
        (
            dialog.fields[2].enable.as_deref(),
            dialog.fields[2].visible.as_deref()
        ),
        (Some("injLayout != 0"), Some("nCylinders == 4"))
    );
    assert_eq!(
        (
            dialog.fields[3].enable.as_deref(),
            dialog.fields[3].visible.as_deref()
        ),
        (Some("enabledDisplay"), Some("visibleDisplay"))
    );
    assert_eq!(
        (
            dialog.fields[4].enable.as_deref(),
            dialog.fields[4].visible.as_deref()
        ),
        (Some("enabledSlider"), Some("visibleSlider"))
    );
}
