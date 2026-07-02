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
    // wueRates occupies bytes [2, 12) → lastOffset resolves to 12.
    let ve = def.constant("veTable").expect("veTable");
    assert_eq!(ve.offset, 12);
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
    // veTable occupies bytes [12, 28) → lastOffset resolves to 28... but
    // `stoich` itself occupies offset 28 (1 byte) first, so ego_min_lambda
    // (declared after stoich) resolves to 29.
    let ego = def.constant("ego_min_lambda").expect("ego_min_lambda");
    assert_eq!(ego.offset, 29);
    assert_eq!(ego.scale, Number::Expr("0.1 / stoich".to_string()));
    assert_eq!(ego.low, Number::Expr("7 / stoich".to_string()));
    assert_eq!(ego.high, Number::Expr("25 / stoich".to_string()));
    assert_eq!(ego.digits, 3);

    // ── `#if CELSIUS`/`#else`/`#endif` gate ─────────────────────────
    // `parse_definition` preprocesses with an empty active-symbol set, so
    // `CELSIUS` is inactive and the `#else` (Fahrenheit) branch survives.
    let coolant_gate = def.constant("coolantGate").expect("coolantGate");
    assert_eq!(coolant_gate.offset, 30);
    assert_eq!(coolant_gate.units, "F");
    assert_eq!(coolant_gate.scale, Number::Lit(1.8));

    // ── string constant ──────────────────────────────────────────────
    let name = def.constant("engineName").expect("engineName");
    assert_eq!(name.offset, 31);
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
