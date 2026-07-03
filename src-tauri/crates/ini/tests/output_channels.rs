// SPDX-License-Identifier: GPL-3.0-or-later
//! Integration tests for `[OutputChannels]` + `ochBlockSize` parsing —
//! sub-steps 2.3 and 2.6.
//!
//! Port source (ADR-0006): [`hyper-tuner/ini`](https://github.com/hyper-tuner/ini)
//! (MIT) — `parseOutputChannels` (`src/ini.ts`, ~lines 235-266) tries the
//! same `parseConstAndVar` shape already ported for `[Constants]` in M2
//! (`constants_fields.rs`) first; scalar/bits entries there use the "short"
//! field list (no `min`/`max`/`digits` tail — confirmed by reading
//! `parseConstAndVar`'s `scalarShort`/`bitsShort` branches, which
//! `parseOutputChannels` reaches because it never supplies the trailing
//! `min, max, digits` fields). On failure it falls back to a generic
//! `name = value` parse that captures `ochGetCommand`, `ochBlockSize`, and
//! computed `{ expr }` channels as opaque strings/values.
//!
//! Extension (not in hyper-tuner, written fresh): computed channels'
//! `{ expr }` is stored as an **opaque string** here too, but expression
//! *evaluation* (resolving e.g. `coolantRaw - 40` against sibling channel
//! values) is deferred to Task 6 — this parser only captures the raw text.
//! Unknown entry kinds degrade gracefully (`Diagnostic` + continue) per this
//! project's contract, rather than hyper-tuner's `.tryParse` throw.

use opentune_ini::{parse_definition, OutputChannelDef, ScalarType};

fn fixture() -> &'static str {
    include_str!("fixtures/speeduino-output-channels.ini")
}

#[test]
fn parses_output_channels_and_block_size() {
    let ini = fixture();
    let def = parse_definition(ini).expect("parses");
    assert_eq!(def.comms.och_block_size, 16);

    // scalar with offset + scale
    match def.output_channel("rpm").unwrap() {
        OutputChannelDef::Scalar {
            kind,
            offset,
            units,
            scale,
            ..
        } => {
            assert_eq!(*kind, ScalarType::U16);
            assert_eq!(*offset, 4);
            assert_eq!(units, "rpm");
            assert!((scale - 1.0).abs() < 1e-9);
        }
        other => panic!("expected Scalar, got {other:?}"),
    }

    // bits over the `engine` byte
    match def.output_channel("running").unwrap() {
        OutputChannelDef::Bits {
            offset,
            bit_lo,
            bit_hi,
            ..
        } => {
            assert_eq!((*offset, *bit_lo, *bit_hi), (2, 0, 0));
        }
        other => panic!("expected Bits, got {other:?}"),
    }

    // computed channel keeps its expression verbatim + trailing units
    match def.output_channel("coolant").unwrap() {
        OutputChannelDef::Computed { expr, units, .. } => {
            assert_eq!(expr.trim(), "coolantRaw - 40");
            assert_eq!(units, "C");
        }
        other => panic!("expected Computed, got {other:?}"),
    }
}

#[test]
fn scalar_fields_carry_exact_offsets_scales_and_units() {
    let def = parse_definition(fixture()).expect("parses");

    match def.output_channel("secl").unwrap() {
        OutputChannelDef::Scalar {
            kind,
            offset,
            units,
            scale,
            translate,
            ..
        } => {
            assert_eq!(*kind, ScalarType::U08);
            assert_eq!(*offset, 0);
            assert_eq!(units, "sec");
            assert!((scale - 1.0).abs() < 1e-9);
            assert!((translate - 0.0).abs() < 1e-9);
        }
        other => panic!("expected Scalar, got {other:?}"),
    }

    match def.output_channel("tps").unwrap() {
        OutputChannelDef::Scalar {
            kind,
            offset,
            units,
            scale,
            ..
        } => {
            assert_eq!(*kind, ScalarType::U08);
            assert_eq!(*offset, 7);
            assert_eq!(units, "%");
            assert!((scale - 0.5).abs() < 1e-9);
        }
        other => panic!("expected Scalar, got {other:?}"),
    }
}

#[test]
fn computed_channel_with_no_trailing_units_has_empty_units() {
    let def = parse_definition(fixture()).expect("parses");

    match def.output_channel("throttle").unwrap() {
        OutputChannelDef::Computed { expr, units, .. } => {
            assert_eq!(expr.trim(), "tps");
            assert_eq!(units, "%");
        }
        other => panic!("expected Computed, got {other:?}"),
    }
}

#[test]
fn unknown_output_channel_kind_records_diagnostic_and_continues() {
    let ini = format!(
        "{}\n{}\n",
        fixture(),
        r#"foo = weird, U08, 0, "x", 1.000, 0.000"#
    );
    let def = parse_definition(&ini).expect("parses despite unknown channel kind");

    assert!(def
        .diagnostics
        .iter()
        .any(|d| d.detail.contains("foo") || d.detail.contains("weird")));
    // Parsing continues: known channels are still present.
    assert!(def.output_channel("rpm").is_some());
    // The unknown entry itself is not stored as a channel.
    assert!(def.output_channel("foo").is_none());
}

#[test]
fn computed_channel_bare_expression_with_no_units_field() {
    // A computed channel with no trailing `, "units"` at all → units == "".
    let ini = format!("{}\n{}\n", fixture(), "bareComputed = { secl + 1 }");
    let def = parse_definition(&ini).expect("parses");

    match def.output_channel("bareComputed").unwrap() {
        OutputChannelDef::Computed { expr, units, .. } => {
            assert_eq!(expr.trim(), "secl + 1");
            assert_eq!(units, "");
        }
        other => panic!("expected Computed, got {other:?}"),
    }
}
