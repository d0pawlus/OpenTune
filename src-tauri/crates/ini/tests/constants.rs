// SPDX-License-Identifier: GPL-3.0-or-later
//! Integration tests for `parse_definition`'s `[Constants]`/pages parsing —
//! sub-steps 1.3 and 1.5.
//!
//! Port source (ADR-0006): [`hyper-tuner/ini`](https://github.com/hyper-tuner/ini)
//! (MIT) — `parseConstants`/`parseConstAndVar` cover `page=N`, scalar/array/
//! bits parsing, offset handling, and the full scalar field tail. Field
//! renames honored: hyper-tuner's internal `transform` → our `translate`,
//! its `min`/`max` → our `low`/`high`.
//!
//! Gap filled (written fresh, not in either reference): `lastOffset`
//! resolution via a running per-page byte counter — absent from both
//! hyper-tuner and `adbancroft/TunerStudioIniParser`. String-type
//! constants reachable from `[Constants]` (not just `[PcVariables]`) are
//! ported structurally from `adbancroft/TunerStudioIniParser`'s
//! `StringVariable`/`ts_ini.lark` grammar (LGPLv3) — field order
//! `name = string, ENCODING, offset, LENGTH` — while independently mapping
//! `ASCII`/length to [`ConstantKind::Text`] to avoid adbancroft's
//! `F32`→`U08` `type_name` typo (verified present in its
//! `type_factory.py`; irrelevant here since strings aren't numeric-typed,
//! but the independent mapping is a deliberate choice, not an oversight).
//!
//! `tests/fixtures/speeduino-constants.ini` is trimmed from the real
//! Speeduino `speeduino.ini` (GPL-3, noisymime/speeduino) — see the
//! fixture's own header comment for the full provenance and the exact
//! constant set it exercises.

use opentune_ini::{parse_definition, ConstantKind, IniError, Number, ScalarType, Shape};

fn fixture() -> &'static str {
    include_str!("fixtures/speeduino-constants.ini")
}

#[test]
fn parses_constants_and_pages() {
    let def = parse_definition(fixture()).expect("fixture should parse");

    // ── pages ────────────────────────────────────────────────────────
    assert_eq!(def.pages.len(), 1);
    assert_eq!(def.pages[0].number, 1);
    assert_eq!(def.pages[0].size, 43);

    // ── scalar (full field list) ────────────────────────────────────
    let ase = def.constant("aseTaperTime").expect("aseTaperTime");
    assert_eq!(ase.page, 1);
    assert_eq!(ase.offset, 0);
    assert_eq!(ase.kind, ConstantKind::Scalar(ScalarType::U08));
    assert_eq!(ase.scale, Number::Lit(0.1));
    assert_eq!(ase.translate, Number::Lit(0.0));
    assert_eq!(ase.units, "S");
    assert_eq!(ase.low, Number::Lit(0.0));
    assert_eq!(ase.high, Number::Lit(25.5));
    assert_eq!(ase.digits, 1);

    // ── bitfield with named options (including "INVALID") ──────────
    let ae_mode = def.constant("aeMode").expect("aeMode");
    assert_eq!(ae_mode.page, 1);
    assert_eq!(ae_mode.offset, 1);
    match &ae_mode.kind {
        ConstantKind::Bits {
            storage,
            bit_lo,
            bit_hi,
            options,
        } => {
            assert_eq!(*storage, ScalarType::U08);
            assert_eq!(*bit_lo, 0);
            assert_eq!(*bit_hi, 1);
            assert_eq!(
                options,
                &vec![
                    "TPS".to_string(),
                    "MAP".to_string(),
                    "INVALID".to_string(),
                    "INVALID".to_string(),
                ]
            );
        }
        other => panic!("expected Bits, got {other:?}"),
    }

    // ── 1-D array ────────────────────────────────────────────────────
    let wue = def.constant("wueRates").expect("wueRates");
    assert_eq!(wue.offset, 2);
    match &wue.kind {
        ConstantKind::Array { elem, shape } => {
            assert_eq!(*elem, ScalarType::U08);
            assert_eq!(*shape, Shape { rows: 10, cols: 1 });
        }
        other => panic!("expected Array, got {other:?}"),
    }

    // ── 2-D table array, offset resolved via `lastOffset` ───────────
    // M4 correction (real speeduino.ini @ 0832dc1d l.640-645/675-679):
    // `lastOffset` resolves to the previous field's START, not the running
    // end — veTable ALIASES wueRates's own offset (2), it does not follow
    // it. (The M2-era expectation of 12 pinned the old, incorrect
    // running-end semantics; `Definition`'s shape is unchanged, only this
    // resolved value is.)
    let ve = def.constant("veTable").expect("veTable");
    assert_eq!(ve.offset, 2);
    match &ve.kind {
        ConstantKind::Array { elem, shape } => {
            assert_eq!(*elem, ScalarType::U08);
            assert_eq!(*shape, Shape { rows: 4, cols: 4 });
        }
        other => panic!("expected Array, got {other:?}"),
    }
    assert_eq!(ve.high, Number::Lit(255.0));

    // ── plain scalar referenced by a later expression ───────────────
    let stoich = def.constant("stoich").expect("stoich");
    assert_eq!(stoich.offset, 28);
    assert_eq!(stoich.scale, Number::Lit(0.1));

    // ── expression-scaled constant, offset via `lastOffset` ─────────
    // M4 correction: `lastOffset` aliases the previous field's START.
    // `stoich` starts at 28, so `ego_min_lambda` (declared right after)
    // resolves to 28 too — the AFR↔Lambda alias shape from the real file
    // (was 29 under the old, incorrect running-end semantics).
    let ego = def.constant("ego_min_lambda").expect("ego_min_lambda");
    assert_eq!(ego.offset, 28);
    assert_eq!(ego.scale, Number::Expr("0.1 / stoich".to_string()));
    assert_eq!(ego.low, Number::Expr("7 / stoich".to_string()));
    assert_eq!(ego.high, Number::Expr("25 / stoich".to_string()));
    assert_eq!(ego.digits, 3);

    // ── `#if CELSIUS`/`#else`/`#endif` gate ─────────────────────────
    // `parse_definition` preprocesses with an empty active-symbol set, so
    // `CELSIUS` is inactive and the `#else` (Fahrenheit) branch survives.
    // M4 correction: `coolantGate`'s `lastOffset` aliases `ego_min_lambda`'s
    // start (28), chaining the alias further (was 30 under the old
    // running-end semantics).
    let coolant_gate = def.constant("coolantGate").expect("coolantGate");
    assert_eq!(coolant_gate.offset, 28);
    assert_eq!(coolant_gate.units, "F");
    assert_eq!(coolant_gate.scale, Number::Lit(1.8));

    // ── string constant ──────────────────────────────────────────────
    // M4 correction: same alias chain, continued (was 31 under the old
    // running-end semantics).
    let name = def.constant("engineName").expect("engineName");
    assert_eq!(name.offset, 28);
    assert_eq!(name.kind, ConstantKind::Text { len: 12 });

    // ── pc_variables (no offset field) ──────────────────────────────
    let rpmhigh = def
        .pc_variables
        .iter()
        .find(|c| c.name == "rpmhigh")
        .expect("rpmhigh pc_variable");
    assert_eq!(rpmhigh.kind, ConstantKind::Scalar(ScalarType::U16));
    assert_eq!(rpmhigh.units, "rpm");
    assert_eq!(rpmhigh.high, Number::Lit(30000.0));
}

#[test]
fn unknown_constant_class_is_a_diagnostic_not_a_hard_error() {
    let ini = "\
[MegaTune]
   signature            = \"test ECU\"
   queryCommand         = \"Q\"
   versionInfo          = \"S\"
   ochGetCommand        = \"r\"
   pageReadCommand      = \"p\"
   pageValueWrite       = \"w\"
   burnCommand          = \"b\"
   blockingFactor       = 121
   blockReadTimeout     = 1000

[Constants]
    nPages   = 1
    pageSize = 4

page = 1
      knownScalar = scalar, U08, 0, \"units\", 1.0, 0.0, 0.0, 255, 0
      weirdThing  = frobnicate, U08, 1, \"units\", 1.0, 0.0, 0.0, 255, 0
";
    let def = parse_definition(ini).expect("parse should continue past unknown class");
    assert!(def.constant("knownScalar").is_some());
    assert!(def.constant("weirdThing").is_none());
    assert!(
        def.diagnostics
            .iter()
            .any(|d| d.section == "Constants" && d.detail.contains("weirdThing")),
        "expected a diagnostic naming the unrecognised constant; got {:?}",
        def.diagnostics
    );
}

#[test]
fn unknown_constant_class_poisons_later_lastoffset_on_the_same_page() {
    // knownScalar (U08 @ 0) advances the page-1 running offset to 1.
    // weirdThing (unrecognised class) cannot be sized, so the running
    // offset can no longer be trusted; the next `lastOffset` constant
    // (staleOffset) must NOT silently resolve to the pre-unknown-line
    // value (1) -- it must be skipped with its own diagnostic instead of
    // desyncing onto the wrong-but-plausible offset.
    let ini = "\
[MegaTune]
   signature            = \"test ECU\"
   queryCommand         = \"Q\"
   versionInfo          = \"S\"
   ochGetCommand        = \"r\"
   pageReadCommand      = \"p\"
   pageValueWrite       = \"w\"
   burnCommand          = \"b\"
   blockingFactor       = 121
   blockReadTimeout     = 1000

[Constants]
    nPages   = 1
    pageSize = 10

page = 1
      knownScalar  = scalar, U08, 0, \"units\", 1.0, 0.0, 0.0, 255, 0
      weirdThing   = frobnicate, U08, 1, \"units\", 1.0, 0.0, 0.0, 255, 0
      staleOffset  = scalar, U08, lastOffset, \"units\", 1.0, 0.0, 0.0, 255, 0
      recovered    = scalar, U08, 5, \"units\", 1.0, 0.0, 0.0, 255, 0
      resumed      = scalar, U08, lastOffset, \"units\", 1.0, 0.0, 0.0, 255, 0
";
    let def = parse_definition(ini).expect("parse should continue past unknown class");

    assert!(def.constant("knownScalar").is_some());
    assert!(
        def.constant("staleOffset").is_none(),
        "a lastOffset constant after an unknown class must be skipped, not desynced"
    );
    assert!(
        def.diagnostics
            .iter()
            .any(|d| d.section == "Constants" && d.detail.contains("staleOffset")),
        "expected a diagnostic naming the poisoned-offset constant; got {:?}",
        def.diagnostics
    );

    // An explicit numeric offset on the same page is unaffected by the
    // poison and re-anchors the running counter for constants after it.
    // M4 correction: `lastOffset` resolves to the previous field's START
    // (aliasing `recovered`, offset 5), not its end (was 6 under the old,
    // incorrect running-end semantics — see `last_offset_is_previous_field_
    // start` / real speeduino.ini @ 0832dc1d).
    let recovered = def.constant("recovered").expect("recovered");
    assert_eq!(recovered.offset, 5);
    let resumed = def.constant("resumed").expect("resumed");
    assert_eq!(
        resumed.offset, 5,
        "lastOffset resolves correctly again once re-anchored by an explicit offset"
    );
}

#[test]
fn pc_variables_support_array_bits_and_string_classes_not_just_scalar() {
    // Wall #3 (discovered running the M4 golden gate, not predicted by the
    // task brief): real speeduino.ini's `[PcVariables]` section (l.50-154 @
    // 0832dc1d) uses `array`/`bits`/`string` classes extensively (e.g.
    // `wueAFR = array, S16, [10], "AFR", 0.1, 0.0, -4.0, 4.0, 1`,
    // `tsCanId = bits, U08, [0:3], "CAN ID 0", ...`,
    // `AUXin00Alias = string, ASCII, 20`), but `parse_constant_line` only
    // wired `("scalar", None)` for the no-offset (`[PcVariables]`) case —
    // every other class fell through to `UnknownClass`. This is a grammar
    // gap in an already-modeled section (`[PcVariables]` scalars already
    // work), so it's fixed here rather than allowlisted.
    let ini = "\
[MegaTune]
   signature            = \"test ECU\"
   queryCommand         = \"Q\"
   versionInfo          = \"S\"
   ochGetCommand        = \"r\"
   pageReadCommand      = \"p\"
   pageValueWrite       = \"w\"
   burnCommand          = \"b\"
   blockingFactor       = 121
   blockReadTimeout     = 1000

[PcVariables]
   wueAFR = array, S16, [10], \"AFR\", 0.1, 0.0, -4.0, 4.0, 1
   tsCanId = bits, U08, [0:3], \"CAN ID 0\", \"CAN ID 1\", \"INVALID\"
   AUXin00Alias = string, ASCII, 20
";
    let def = parse_definition(ini).expect("PcVariables array/bits/string must parse");
    assert!(
        def.diagnostics.is_empty(),
        "expected zero diagnostics; got {:?}",
        def.diagnostics
    );

    let wue_afr = def
        .pc_variables
        .iter()
        .find(|c| c.name == "wueAFR")
        .expect("wueAFR pc_variable");
    match &wue_afr.kind {
        ConstantKind::Array { elem, shape } => {
            assert_eq!(*elem, ScalarType::S16);
            assert_eq!(*shape, Shape { rows: 10, cols: 1 });
        }
        other => panic!("expected Array, got {other:?}"),
    }
    assert_eq!(wue_afr.units, "AFR");
    assert_eq!(wue_afr.low, Number::Lit(-4.0));
    assert_eq!(wue_afr.high, Number::Lit(4.0));

    let ts_can_id = def
        .pc_variables
        .iter()
        .find(|c| c.name == "tsCanId")
        .expect("tsCanId pc_variable");
    match &ts_can_id.kind {
        ConstantKind::Bits {
            storage,
            bit_lo,
            bit_hi,
            options,
        } => {
            assert_eq!(*storage, ScalarType::U08);
            assert_eq!(*bit_lo, 0);
            assert_eq!(*bit_hi, 3);
            assert_eq!(
                options,
                &vec![
                    "CAN ID 0".to_string(),
                    "CAN ID 1".to_string(),
                    "INVALID".to_string(),
                ]
            );
        }
        other => panic!("expected Bits, got {other:?}"),
    }

    let aux_alias = def
        .pc_variables
        .iter()
        .find(|c| c.name == "AUXin00Alias")
        .expect("AUXin00Alias pc_variable");
    assert_eq!(aux_alias.kind, ConstantKind::Text { len: 20 });
}

#[test]
fn scattered_metadata_keys_in_constants_are_silently_skipped() {
    // Real speeduino.ini l.240-274 carries TS metadata / comms keys inside
    // the `[Constants]` header block, before any `page = N` line. These are
    // neither an unknown constant `class` nor page data — they must produce
    // zero diagnostics and must not poison the running offset counter for
    // the page that follows.
    let ini = "\
[MegaTune]
   signature            = \"test ECU\"
   queryCommand         = \"Q\"
   versionInfo          = \"S\"
   ochGetCommand        = \"r\"
   pageReadCommand      = \"p\"
   pageValueWrite       = \"w\"
   burnCommand          = \"b\"
   blockingFactor       = 121
   blockReadTimeout     = 1000

[Constants]
    nPages              = 1
    pageSize            = 10
    pageReadCommand     = \"p%2i%2o%2c\", \"p%2i%2o%2c\"

page = 1
      first  = scalar, U08, 0, \"units\", 1.0, 0.0, 0.0, 255, 0
      second = scalar, U08, lastOffset, \"units\", 1.0, 0.0, 0.0, 255, 0
";
    let def = parse_definition(ini).expect("metadata keys must not break parsing");
    assert!(
        def.diagnostics.is_empty(),
        "expected zero diagnostics; got {:?}",
        def.diagnostics
    );
    assert!(def.constant("first").is_some());
    assert!(
        def.constant("second").is_some(),
        "a following lastOffset constant must still resolve (not poisoned) \
         after the metadata keys; diagnostics: {:?}",
        def.diagnostics
    );
}

#[test]
fn last_offset_is_previous_field_start() {
    // reference/speeduino.ini @ 0832dc1d l.640-645 + l.675-679: every real
    // `lastOffset` use ALIASES the immediately-preceding field (AFR↔Lambda
    // views over the same bytes). TS semantics: lastOffset = the previous
    // field's START offset, not the running end.
    let ini = r#"
[MegaTune]
   signature      = "test"
   queryCommand   = "Q"
   versionInfo    = "S"
   ochGetCommand  = "r"
   pageReadCommand = "p%2i%2o%2c"
   pageValueWrite = "M%2i%2o%2c%v"
   burnCommand    = "b%2i"
   blockingFactor = 121
   blockReadTimeout = 2000

[Constants]
    pageSize = 288

page = 1
      lambdaTable = array,  U08,          0, [16x16], "Lambda", 0.006, 0.0, 0.0, 2.0, 3
      afrTable    = array,  U08, lastOffset, [16x16], "AFR",    0.1,   0.0, 7.0, 25.5, 1
      rpmBinsAFR  = array,  U08, 256, [16], "RPM", 100.0, 0.0, 100.0, 25500.0, 0
      ego_min_afr    = scalar, U08, 272, "AFR", 0.1, 0.0, 7.0, 25.0, 1
      ego_min_lambda = scalar, U08, lastOffset, "Lambda", 0.006, 0.0, 0.0, 2.0, 3
"#;
    let def = opentune_ini::parse_definition(ini).expect("aliased page must parse");
    let lambda = def.constant("lambdaTable").unwrap();
    let afr = def.constant("afrTable").unwrap();
    assert_eq!(
        (lambda.offset, afr.offset),
        (0, 0),
        "afrTable aliases lambdaTable"
    );
    let e_afr = def.constant("ego_min_afr").unwrap();
    let e_lambda = def.constant("ego_min_lambda").unwrap();
    assert_eq!(
        e_lambda.offset, e_afr.offset,
        "scalar alias shares its byte"
    );
    assert!(
        def.diagnostics.is_empty(),
        "no diagnostics: {:?}",
        def.diagnostics
    );
}

#[test]
fn offset_beyond_declared_page_size_is_a_hard_error() {
    let ini = "\
[MegaTune]
   signature            = \"test ECU\"
   queryCommand         = \"Q\"
   versionInfo          = \"S\"
   ochGetCommand        = \"r\"
   pageReadCommand      = \"p\"
   pageValueWrite       = \"w\"
   burnCommand          = \"b\"
   blockingFactor       = 121
   blockReadTimeout     = 1000

[Constants]
    nPages   = 1
    pageSize = 2

page = 1
      tooFar = scalar, U16, 4, \"units\", 1.0, 0.0, 0.0, 255, 0
";
    let err = parse_definition(ini).expect_err("offset beyond page size must be a hard error");
    assert!(matches!(err, IniError::InvalidValue { .. }));
}

#[test]
fn scalar_offset_overflow_degrades_to_diagnostic_not_panic() {
    // M4 final-review fix wave item 1: `def.offset + size` used unchecked
    // `usize` arithmetic. This offset is `usize::MAX` (the exact adversarial
    // line from the finding) — `offset + size` overflows. Must degrade to a
    // Diagnostic + skip, never panic (debug) or silently wrap (release).
    let ini = "\
[MegaTune]
   signature            = \"test ECU\"
   queryCommand         = \"Q\"
   versionInfo          = \"S\"
   ochGetCommand        = \"r\"
   pageReadCommand      = \"p\"
   pageValueWrite       = \"w\"
   burnCommand          = \"b\"
   blockingFactor       = 121
   blockReadTimeout     = 1000

[Constants]
    nPages   = 1
    pageSize = 128

page = 1
a = scalar, U08, 18446744073709551615, \"u\", 1, 0, 0, 255, 0
";
    let def =
        parse_definition(ini).expect("an overflowing offset must degrade, never panic/hard-error");
    assert!(
        def.constant("a").is_none(),
        "the overflowing constant must be skipped, not stored"
    );
    assert!(
        def.diagnostics
            .iter()
            .any(|d| d.section == "Constants" && d.detail.contains('a')),
        "expected a diagnostic naming `a`; got {:?}",
        def.diagnostics
    );
}

#[test]
fn array_shape_overflow_degrades_to_diagnostic_not_panic() {
    // Same item, the `constant_byte_size` half: `scalar_width * rows * cols`
    // overflows for this adversarial shape (the exact line from the finding).
    let ini = "\
[MegaTune]
   signature            = \"test ECU\"
   queryCommand         = \"Q\"
   versionInfo          = \"S\"
   ochGetCommand        = \"r\"
   pageReadCommand      = \"p\"
   pageValueWrite       = \"w\"
   burnCommand          = \"b\"
   blockingFactor       = 121
   blockReadTimeout     = 1000

[Constants]
    nPages   = 1
    pageSize = 128

page = 1
a = array, U32, 0, [4294967295x4294967295], \"u\", 1, 0, 0, 255, 0
";
    let def = parse_definition(ini)
        .expect("an overflowing array shape must degrade, never panic/hard-error");
    assert!(
        def.constant("a").is_none(),
        "the overflowing constant must be skipped, not stored"
    );
    assert!(
        def.diagnostics
            .iter()
            .any(|d| d.section == "Constants" && d.detail.contains('a')),
        "expected a diagnostic naming `a`; got {:?}",
        def.diagnostics
    );
}

#[test]
fn page_size_value_exceeding_the_max_is_a_hard_error() {
    // Security finding: `pageSize` fed `vec![0u8; size]` in
    // crates/simulator/src/memory.rs and crates/model/src/tune.rs with no
    // upper bound — `pageSize = 99999999999` (the exact adversarial line
    // from the finding) triggers an allocation abort (`handle_alloc_error`,
    // uncatchable, kills the app) before any ECU handshake. Must be a hard
    // parse error instead, same style as `offset_beyond_declared_page_size_
    // is_a_hard_error`.
    let ini = "\
[MegaTune]
   signature            = \"test ECU\"
   queryCommand         = \"Q\"
   versionInfo          = \"S\"
   ochGetCommand        = \"r\"
   pageReadCommand      = \"p\"
   pageValueWrite       = \"w\"
   burnCommand          = \"b\"
   blockingFactor       = 121
   blockReadTimeout     = 1000

[Constants]
    nPages   = 1
    pageSize = 99999999999
";
    let err = parse_definition(ini).expect_err("oversized pageSize must be a hard error");
    assert!(matches!(err, IniError::InvalidValue { .. }));
}

#[test]
fn page_size_list_longer_than_the_max_count_is_a_hard_error() {
    // Companion to the oversized-value case: `pageSize = a,b,c,...` splits
    // on `,` with no bound on the number of entries either. A page count
    // far beyond any real INI must also hard-error, not silently allocate
    // one `PageDef` per token.
    let too_many = (0..65).map(|_| "4").collect::<Vec<_>>().join(",");
    let ini = format!(
        "\
[MegaTune]
   signature            = \"test ECU\"
   queryCommand         = \"Q\"
   versionInfo          = \"S\"
   ochGetCommand        = \"r\"
   pageReadCommand      = \"p\"
   pageValueWrite       = \"w\"
   burnCommand          = \"b\"
   blockingFactor       = 121
   blockReadTimeout     = 1000

[Constants]
    nPages   = 65
    pageSize = {too_many}
"
    );
    let err =
        parse_definition(&ini).expect_err("a pageSize list of 65 entries must be a hard error");
    assert!(matches!(err, IniError::InvalidValue { .. }));
}

#[test]
fn page_size_non_numeric_token_is_a_hard_error() {
    // Pins the chosen behavior for a garbage token: hard error, not the
    // previous silent `unwrap_or(0)` zero-size page.
    let ini = "\
[MegaTune]
   signature            = \"test ECU\"
   queryCommand         = \"Q\"
   versionInfo          = \"S\"
   ochGetCommand        = \"r\"
   pageReadCommand      = \"p\"
   pageValueWrite       = \"w\"
   burnCommand          = \"b\"
   blockingFactor       = 121
   blockReadTimeout     = 1000

[Constants]
    nPages   = 1
    pageSize = not_a_number
";
    let err = parse_definition(ini).expect_err("a non-numeric pageSize token must be a hard error");
    assert!(matches!(err, IniError::InvalidValue { .. }));
}

#[test]
fn page_size_multi_page_list_still_parses_with_correct_sizes() {
    // Regression guard: a sane multi-page declaration, well within both
    // bounds, must still parse to the correct per-page sizes.
    let ini = "\
[MegaTune]
   signature            = \"test ECU\"
   queryCommand         = \"Q\"
   versionInfo          = \"S\"
   ochGetCommand        = \"r\"
   pageReadCommand      = \"p\"
   pageValueWrite       = \"w\"
   burnCommand          = \"b\"
   blockingFactor       = 121
   blockReadTimeout     = 1000

[Constants]
    nPages   = 3
    pageSize = 288,64,288
";
    let def = parse_definition(ini).expect("a sane pageSize list must parse");
    assert_eq!(def.pages.len(), 3);
    assert_eq!(
        def.pages[0],
        opentune_ini::PageDef {
            number: 1,
            size: 288
        }
    );
    assert_eq!(
        def.pages[1],
        opentune_ini::PageDef {
            number: 2,
            size: 64
        }
    );
    assert_eq!(
        def.pages[2],
        opentune_ini::PageDef {
            number: 3,
            size: 288
        }
    );
}

#[test]
fn endianness_big_in_constants_section_overrides_comms_default() {
    // [MegaTune] omits `endianness` entirely (so `parse_comms` falls back to
    // its own default, `Little`); only [Constants] declares `big`. This
    // discriminates the Constants-section endianness override from the
    // already-covered M1 comms-section parsing (`comms_settings_models_
    // endianness_and_envelope` in contract.rs), which would pass this
    // assertion even if the override were a no-op.
    let ini = "\
[MegaTune]
   signature            = \"test ECU\"
   queryCommand         = \"Q\"
   versionInfo          = \"S\"
   ochGetCommand        = \"r\"
   pageReadCommand      = \"p\"
   pageValueWrite       = \"w\"
   burnCommand          = \"b\"
   blockingFactor       = 121
   blockReadTimeout     = 1000

[Constants]
    endianness = big
    nPages     = 1
    pageSize   = 2

page = 1
      value = scalar, U16, 0, \"units\", 1.0, 0.0, 0.0, 255, 0
";
    let def = parse_definition(ini).expect("should parse with big endianness");
    assert_eq!(def.comms.endianness, opentune_ini::Endianness::Big);
}

// NOTE (M2/M3-review → main reconcile, 2026-07-11): the review's INI-hardening
// tests (undeclared-page rejection, strict page/pageSize parsing, empty-field
// defaults, hard-error offset/shape overflow) were dropped when merging onto
// main. M4 addresses the panic finding via a *lenient* parser — offset/shape
// overflow becomes an `OffsetCheck::Overflow` diagnostic + skip
// (`constants_parser.rs`), not a hard error — which is the shipped, real-INI-
// tuned philosophy. Re-introducing strict rejection here would fight that and
// risk real speeduino.ini ingestion. Follow-up if strict validation is wanted:
// undeclared-page/empty-numeric (`Number::Expr("")`) handling.
//
// UPDATE (CRITICAL security finding, 2026-07-13): the "bad-pageSize" item
// above shipped after all — `page_size_*` tests earlier in this file —
// scoped narrowly to the allocation trust boundary (unparseable token,
// oversized single page, oversized page count all hard-error in
// `parse_page_sizes`), not full strict pageSize parsing in general. The
// bounds (1 MiB / 64 pages) sit far above every real fixture in this repo,
// so this does not reopen the real-INI-ingestion risk noted above.
