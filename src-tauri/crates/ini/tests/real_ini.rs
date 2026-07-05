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
/// dialog widgets with no frozen representation, menu grouping, and any
/// further entries added ONLY together with an m4-decisions record.
///
/// See `docs/notes/m4-decisions.md` for the one-line justification behind
/// every entry below `groupChildMenu` (added running this gate for real;
/// the four above were anticipated by the Task 1 brief and are exercised
/// by real `[UserDefined]` `commandButton`/`settingSelector`/`groupMenu`/
/// `groupChildMenu` lines, e.g. l.2014-2019, l.2689, l.3279).
const ALLOWED_DIAGNOSTICS: &[&str] = &[
    "commandButton",
    "settingSelector",
    "groupMenu",
    "groupChildMenu",
    // Dialog widgets with no frozen `FieldKind` representation (M2's
    // `ui.rs` shape is frozen; Task 2+ scope to extend it).
    "`settingOption`", // named preset consumed by settingSelector, not itself bindable
    "indicator",       // status-light widget (also covers `indicatorPanel`)
    "`text`",          // static help/informational text block
    "`graphLine`",     // embedded live-graph series definition
    "`liveGraph`",     // embedded live-graph widget container
    "`help`",          // help-topic link, informational only
    "`webHelp`",       // web help link, informational only
    "`gauge`",         // embedded gauge widget referenced inside a dialog panel
    // Pre-existing M2 degrade path (not a new gap): a label-only
    // `displayOnlyField` used as an inline dialog comment/spacer, with no
    // bound constant name (real file l.2962, l.3442).
    "displayOnlyField has no bound constant name",
    // Curve axis names resolve only against `[Constants]`
    // (`parse_ui(&preprocessed, &parsed.constants)`); `wueAFR`/
    // `wueRecommended` are `[PcVariables]`-scoped (warmup analyzer curves,
    // real file l.4913/l.4921). Widening axis resolution to also search
    // `pc_variables` is Task 2+ grammar scope, not a Task 1 wall.
    "`wueAFR`",
    "`wueRecommended`",
    // Real-file quirk: `systemTempGauge`'s Fahrenheit branch (l.5262) is
    // missing the commas between `systemTemp`, `"System Temp"`, and `"F"`
    // — an upstream INI typo, not a parser gap. Same tolerance spirit as
    // the `blockingFactor` `[121, 251]` assertion above.
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
