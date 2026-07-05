// SPDX-License-Identifier: GPL-3.0-or-later
//! Contract tests for the `opentune-ini` M1 comms slice and the M2/M3
//! `Definition` seam.
//!
//! These freeze the shape of [`CommsSettings`] (M1) and [`Definition`]
//! (M2, extended in M3 with output channels / gauges / frontpage) so
//! downstream agents build against fixed structs. They construct the
//! types directly rather than calling the `todo!()` parsers.

use opentune_ini::{
    CommsSettings, ConstantDef, ConstantKind, Definition, DialogDef, DialogField, Endianness,
    EnvelopeFormat, FieldKind, FrontPageDef, GaugeDef, IndicatorDef, IniError, MenuDef, MenuItem,
    Number, OutputChannelDef, PageDef, ScalarType,
};

/// A representative Speeduino comms block, field names mirroring the real INI.
fn speeduino_comms() -> CommsSettings {
    CommsSettings {
        signature: "speeduino 202504-dev".to_string(),
        query_command: "Q".to_string(),
        version_info: "S".to_string(),
        och_get_command: "r".to_string(),
        page_read_command: "p%2i%2o%2c".to_string(),
        page_value_write: "M%2i%2o%2c%v".to_string(),
        burn_command: "b%2i".to_string(),
        blocking_factor: 251,
        page_activation_delay_ms: 10,
        block_read_timeout_ms: 2_000,
        inter_write_delay_ms: 10,
        endianness: Endianness::Little,
        envelope: EnvelopeFormat::MsEnvelope10,
        och_block_size: 139,
    }
}

#[test]
fn comms_settings_captures_signature_and_query() {
    let c = speeduino_comms();
    assert_eq!(c.signature, "speeduino 202504-dev");
    assert_eq!(c.query_command, "Q");
    assert_eq!(c.version_info, "S");
}

#[test]
fn comms_settings_keeps_raw_command_templates() {
    // The `ini` crate must not expand %2i/%2o/%2c/%v — that is the protocol
    // crate's job. The contract is: store the template verbatim.
    let c = speeduino_comms();
    assert_eq!(c.page_read_command, "p%2i%2o%2c");
    assert_eq!(c.page_value_write, "M%2i%2o%2c%v");
    assert_eq!(c.burn_command, "b%2i");
}

#[test]
fn comms_settings_models_endianness_and_envelope() {
    let c = speeduino_comms();
    assert_eq!(c.endianness, Endianness::Little);
    assert_eq!(c.envelope, EnvelopeFormat::MsEnvelope10);
}

#[test]
fn comms_settings_captures_och_block_size() {
    // M3: `ochBlockSize` is the %2c count for a full realtime frame read.
    let c = speeduino_comms();
    assert_eq!(c.och_block_size, 139);
}

#[test]
fn ini_error_reports_missing_key() {
    let e = IniError::MissingKey("signature".to_string());
    assert!(e.to_string().contains("signature"));
}

// ── M2: Definition contract ─────────────────────────────────────────────────

/// A hand-built `Definition` with one page, one scalar constant, and one
/// dialog exposing that constant — enough to pin the shape without a parser.
fn hand_built_definition() -> Definition {
    Definition {
        comms: speeduino_comms(),
        pages: vec![PageDef {
            number: 1,
            size: 128,
        }],
        constants: vec![ConstantDef {
            name: "rpmk".to_string(),
            page: 1,
            offset: 0,
            kind: ConstantKind::Scalar(ScalarType::U16),
            scale: Number::Lit(1.0),
            translate: Number::Lit(0.0),
            units: "RPM".to_string(),
            low: Number::Lit(0.0),
            high: Number::Lit(10_000.0),
            digits: 0,
        }],
        pc_variables: vec![],
        menus: vec![MenuDef {
            label: "Tuning".to_string(),
            items: vec![MenuItem {
                label: "Engine Speed".to_string(),
                dialog: "engineSpeedDialog".to_string(),
            }],
        }],
        dialogs: vec![DialogDef {
            name: "engineSpeedDialog".to_string(),
            title: "Engine Speed".to_string(),
            fields: vec![DialogField {
                kind: FieldKind::Constant("rpmk".to_string()),
                visible: None,
                enable: None,
            }],
        }],
        tables: vec![],
        curves: vec![],
        diagnostics: vec![],
        output_channels: vec![OutputChannelDef::Scalar {
            name: "map".to_string(),
            kind: ScalarType::U16,
            offset: 4,
            units: "kpa".to_string(),
            scale: 1.0,
            translate: 0.0,
        }],
        gauges: vec![GaugeDef {
            name: "tachometer".to_string(),
            channel: "rpm".to_string(),
            title: "Engine Speed".to_string(),
            units: "RPM".to_string(),
            low: Number::Lit(0.0),
            high: Number::Lit(8_000.0),
            lo_danger: Number::Lit(0.0),
            lo_warn: Number::Lit(0.0),
            hi_warn: Number::Lit(6_500.0),
            hi_danger: Number::Lit(7_500.0),
            value_digits: 0,
            label_digits: 0,
            category: "Engine".to_string(),
        }],
        frontpage: FrontPageDef {
            gauge_slots: vec!["tachometer".to_string()],
            indicators: vec![IndicatorDef {
                expr: "running".to_string(),
                off_label: "Stopped".to_string(),
                on_label: "Running".to_string(),
                off_bg: "black".to_string(),
                off_fg: "white".to_string(),
                on_bg: "green".to_string(),
                on_fg: "black".to_string(),
            }],
        },
    }
}

#[test]
fn definition_constant_finds_hand_built_constant_by_name() {
    let def = hand_built_definition();
    let found = def.constant("rpmk").expect("rpmk must be found");
    assert_eq!(found.name, "rpmk");
    assert_eq!(found.page, 1);
    assert_eq!(found.kind, ConstantKind::Scalar(ScalarType::U16));
}

#[test]
fn definition_constant_returns_none_for_unknown_name() {
    let def = hand_built_definition();
    assert!(def.constant("does_not_exist").is_none());
}

#[test]
fn definition_pages_and_dialogs_are_reachable() {
    let def = hand_built_definition();
    assert_eq!(def.pages.len(), 1);
    assert_eq!(def.pages[0].size, 128);
    assert_eq!(def.dialogs.len(), 1);
    assert!(matches!(
        &def.dialogs[0].fields[0].kind,
        FieldKind::Constant(name) if name == "rpmk"
    ));
}

// ── M3: output channels / gauges / frontpage contract ───────────────────────

#[test]
fn definition_output_channel_finds_hand_built_channel_by_name() {
    let def = hand_built_definition();
    let found = def.output_channel("map").expect("map must be found");
    assert_eq!(found.name(), "map");
    assert!(matches!(
        found,
        OutputChannelDef::Scalar {
            kind: ScalarType::U16,
            offset: 4,
            ..
        }
    ));
}

#[test]
fn definition_output_channel_returns_none_for_unknown_name() {
    let def = hand_built_definition();
    assert!(def.output_channel("does_not_exist").is_none());
}

#[test]
fn output_channel_def_name_covers_all_variants() {
    let bits = OutputChannelDef::Bits {
        name: "running".to_string(),
        storage: ScalarType::U08,
        offset: 2,
        bit_lo: 0,
        bit_hi: 0,
    };
    let computed = OutputChannelDef::Computed {
        name: "coolant".to_string(),
        expr: "coolantRaw - 40".to_string(),
        units: String::new(),
    };
    assert_eq!(bits.name(), "running");
    assert_eq!(computed.name(), "coolant");
}

#[test]
fn definition_gauges_and_frontpage_are_reachable() {
    let def = hand_built_definition();
    assert_eq!(def.gauges.len(), 1);
    assert_eq!(def.gauges[0].channel, "rpm");
    assert_eq!(def.gauges[0].hi_warn, Number::Lit(6_500.0));
    assert_eq!(def.frontpage.gauge_slots, vec!["tachometer".to_string()]);
    assert_eq!(def.frontpage.indicators.len(), 1);
    assert_eq!(def.frontpage.indicators[0].expr, "running");
    assert_eq!(def.frontpage.indicators[0].on_bg, "green");
}

#[test]
fn parse_definition_defaults_m3_sections_to_empty() {
    // The M3 section parsers are stubs (Tasks 2–3 fill them); until then a
    // parsed Definition must carry empty collections, not garbage.
    let ini = r#"
[MegaTune]
signature = "speeduino 202504-dev"
queryCommand = "Q"
versionInfo = "S"
ochGetCommand = "A"
pageReadCommand = "p%2i%2o%2c"
pageValueWrite = "M%2i%2o%2c%v"
burnCommand = "b%2i"
blockingFactor = 251
blockReadTimeout = 2000
"#;
    let def = opentune_ini::parse_definition(ini).expect("parse must succeed");
    assert_eq!(def.comms.och_block_size, 0, "Task 2 fills ochBlockSize");
    assert!(def.output_channels.is_empty());
    assert!(def.gauges.is_empty());
    assert!(def.frontpage.gauge_slots.is_empty());
    assert!(def.frontpage.indicators.is_empty());
}
