// SPDX-License-Identifier: GPL-3.0-or-later
//! Contract tests for the `opentune-ini` M1 comms slice and M2 `Definition`
//! seam.
//!
//! These freeze the shape of [`CommsSettings`] (M1) and [`Definition`] (M2)
//! so downstream agents build against fixed structs. They construct the
//! types directly rather than calling the `todo!()` parsers.

use opentune_ini::{
    CommsSettings, ConstantDef, ConstantKind, Definition, DialogDef, DialogField, Endianness,
    EnvelopeFormat, FieldKind, IniError, MenuDef, MenuItem, Number, PageDef, ScalarType,
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
