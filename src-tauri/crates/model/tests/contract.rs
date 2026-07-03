// SPDX-License-Identifier: GPL-3.0-or-later
//! Contract tests for the `opentune-model` M2 `Tune` seam.
//!
//! These freeze the shape and construction behaviour of [`Tune`] against a
//! hand-built [`Definition`], so downstream agents build against a fixed
//! type before `parse_definition`/`load_page`/`get`/`set` are implemented.

use std::sync::Arc;

use opentune_ini::{
    CommsSettings, ConstantDef, ConstantKind, Definition, Endianness, EnvelopeFormat, FrontPageDef,
    Number, PageDef, ScalarType,
};
use opentune_model::Tune;

fn hand_built_comms() -> CommsSettings {
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
        och_block_size: 0,
    }
}

/// A hand-built `Definition` with one page and one scalar constant — enough
/// to pin `Tune`'s shape without a parser.
fn hand_built_definition() -> Definition {
    Definition {
        comms: hand_built_comms(),
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
        menus: vec![],
        dialogs: vec![],
        tables: vec![],
        curves: vec![],
        diagnostics: vec![],
        output_channels: Vec::new(),
        gauges: Vec::new(),
        frontpage: FrontPageDef {
            gauge_slots: Vec::new(),
            indicators: Vec::new(),
        },
    }
}

#[test]
fn new_tune_is_not_dirty() {
    let def = Arc::new(hand_built_definition());
    let tune = Tune::new(def);
    assert!(!tune.is_dirty());
}

#[test]
fn new_tune_zeroes_pages_sized_from_definition() {
    let def = Arc::new(hand_built_definition());
    let tune = Tune::new(Arc::clone(&def));

    let bytes = tune.page_bytes(1);
    assert_eq!(bytes.len(), def.pages[0].size);
    assert!(bytes.iter().all(|&b| b == 0));
}
