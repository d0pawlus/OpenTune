// SPDX-License-Identifier: GPL-3.0-or-later
//! M4 golden gate: the FULL, UNMODIFIED real speeduino.ini must parse with
//! zero non-allowlisted diagnostics.
//!
//! Fixture provenance: `reference/speeduino.ini` from noisymime/speeduino @
//! 0832dc1d25b108cf33b30167284c44e3edd3d35a (GPL-3.0, vendored byte-identical
//! — license-compatible with this GPL-3.0-or-later crate).
//!
//! Allowlist rule: every diagnostic must match a RECORDED-DEFERRED construct
//! below. A new, unexplained diagnostic is a parser gap — fix the parser or
//! record the deferral in docs/notes/m4-decisions.md; NEVER widen this list
//! silently.
use opentune_ini::parse_definition;

const REAL_INI: &str = include_str!("fixtures/speeduino-real-0832dc1d.ini");

/// Substrings identifying recorded-deferred constructs (M2/M4 decisions):
/// dialog widgets with no frozen representation, and any further entries
/// added ONLY together with an m4-decisions record.
///
/// `groupMenu`/`groupChildMenu` were anticipated by the Task 1 brief (real
/// file does use them, l.2014-2019) but are deliberately NOT listed here:
/// `ui_parser.rs`'s `parse_menu_line` already tolerates them silently (no
/// representable `MenuItem` target) and they never produce a `Diagnostic` —
/// 0 matches, confirmed by running this gate (M4 Task 2 cleanup). A dead
/// allowlist entry only hides a future real gap under a name that matches
/// nothing, so it was dropped rather than kept "just in case".
///
/// See `docs/notes/m4-decisions.md` for the one-line justification behind
/// every entry below.
const ALLOWED_DIAGNOSTICS: &[&str] = &[
    "commandButton",
    "settingSelector",
    // Dialog widgets with no frozen `FieldKind` representation (M2's
    // `ui.rs` shape is frozen; a future task's scope to extend it).
    "`settingOption`", // named preset consumed by settingSelector, not itself bindable
    // Split from a single bare `indicator` substring (M4 Task 2 cleanup):
    // the bare form could accidentally swallow an unrelated future
    // `[Constants]` diagnostic naming a `...Indicator...` constant. These
    // two precise, backtick-delimited forms are disjoint (an
    // `indicatorPanel` keyword's detail never also contains the exact
    // substring `` `indicator` ``) and together still cover all 7 real-file
    // occurrences (6× `indicator`, 1× `indicatorPanel`, l.3195-3201).
    "`indicator`",      // status-light widget
    "`indicatorPanel`", // status-light widget GROUP container
    "`text`",           // static help/informational text block
    "`graphLine`",      // embedded live-graph series definition
    "`liveGraph`",      // embedded live-graph widget container
    "`help`",           // help-topic link, informational only
    "`webHelp`",        // web help link, informational only
    "`gauge`",          // embedded gauge widget referenced inside a dialog panel
    // Pre-existing M2 degrade path (not a new gap): a label-only
    // `displayOnlyField` used as an inline dialog comment/spacer, with no
    // bound constant name (real file l.2962, l.3442).
    "displayOnlyField has no bound constant name",
    // Real-file quirk: `systemTempGauge`'s Fahrenheit branch (l.5262) is
    // missing the commas between `systemTemp`, `"System Temp"`, and `"F"`
    // — an upstream INI typo, not a parser gap, and out of M4 Task 2's
    // [TableEditor]/[CurveEditor]/[VeAnalyze] grammar scope
    // (`gauges_parser.rs` untouched). Same tolerance spirit as the
    // `blockingFactor` `[121, 251]` assertion above.
    "`systemTempGauge`",
];

#[test]
fn real_speeduino_ini_parses_diagnostic_clean() {
    let def = parse_definition(REAL_INI).expect("real INI must parse");

    // Wall #1 closed: scattered comms fully resolved.
    assert_eq!(def.comms.signature, "speeduino 202504-dev");
    assert_eq!(def.comms.och_get_command, r"r\$tsCanId\x30%2o%2c");
    assert_eq!(def.comms.page_read_command, "p%2i%2o%2c");
    assert_eq!(def.comms.page_value_write, "M%2i%2o%2c%v");
    assert_eq!(def.comms.och_block_size, 139);
    assert!([121, 251].contains(&def.comms.blocking_factor));
    assert_eq!(def.pages.len(), 15);
    assert_eq!(def.pages[4].size, 288); // page 5

    // Wall #2 closed: all five lastOffset uses alias their predecessor.
    for (alias, original) in [
        ("afrTable", "lambdaTable"),
        ("ego_min_lambda", "ego_min_afr"),
        ("ego_max_lambda", "ego_max_afr"),
        ("afrProtectDeviation", "afrProtectDeviationLambda"),
        ("n2o_maxLambda", "n2o_maxAFR"),
    ] {
        let a = def
            .constant(alias)
            .unwrap_or_else(|| panic!("missing {alias}"));
        let o = def
            .constant(original)
            .unwrap_or_else(|| panic!("missing {original}"));
        assert_eq!(
            (a.page, a.offset),
            (o.page, o.offset),
            "{alias} must alias {original}"
        );
    }
    let afr = def.constant("afrTable").unwrap();
    assert_eq!((afr.page, afr.offset), (5, 0));

    // The M4 payload sections exist (parsed fully in Task 2; here just present).
    assert!(
        def.output_channels.len() > 100,
        "got {}",
        def.output_channels.len()
    );
    assert!(!def.tables.is_empty() && !def.curves.is_empty());
    assert!(!def.gauges.is_empty());

    // Task 2: full table/curve grammar against the real file.
    let ve = def.table("veTable1Tbl").expect("veTable1Tbl");
    assert_eq!(ve.page, 2);
    assert_eq!(
        (ve.x_channel.as_str(), ve.y_channel.as_str()),
        ("rpm", "fuelLoad")
    );
    assert_eq!(ve.title, "VE Table");
    assert_eq!(ve.grid_orient, vec![250.0, 0.0, 340.0]);
    assert_eq!(ve.z, "veTable");
    let dwell = def.curve("dwell_correction_curve").expect("dwell curve");
    assert!(!dwell.title.is_empty());
    assert!(dwell.x_axis.is_some() && dwell.y_axis.is_some());
    // Task 2: [VeAnalyze] binding (the #else / AFR branch wins by default).
    let va = def.ve_analyze.as_ref().expect("[VeAnalyze]");
    assert_eq!(va.maps[0].table, "veTable1Tbl");
    assert_eq!(va.maps[0].lambda_channel, "afr");
    assert!(va.filters.len() >= 9, "got {}", va.filters.len());

    // Diagnostic-clean modulo the recorded allowlist.
    let unexpected: Vec<_> = def
        .diagnostics
        .iter()
        .filter(|d| !ALLOWED_DIAGNOSTICS.iter().any(|p| d.detail.contains(p)))
        .collect();
    assert!(
        unexpected.is_empty(),
        "unexplained diagnostics:\n{unexpected:#?}"
    );
}
