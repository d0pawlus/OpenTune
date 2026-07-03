// SPDX-License-Identifier: GPL-3.0-or-later
//! Shared test fixtures for the `Tune` test binaries (`tune.rs`,
//! `tune_state.rs`, and Task 8's `diff.rs`).
//!
//! Each `tests/*.rs` file compiles this module into its own separate test
//! binary, so a helper unused by one binary (e.g. `load1` in `diff.rs`) is
//! flagged as dead code there even though other binaries use it; allowed
//! crate-wide here rather than per-binary.
#![allow(dead_code)]

use std::sync::Arc;

use opentune_ini::{
    CommsSettings, ConstantDef, ConstantKind, Definition, Endianness, EnvelopeFormat, FrontPageDef,
    Number, PageDef, ScalarType,
};
use opentune_model::Tune;

pub const PAGE_SIZE: usize = 64;

pub fn comms(endianness: Endianness) -> CommsSettings {
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
        endianness,
        envelope: EnvelopeFormat::MsEnvelope10,
        och_block_size: 0,
    }
}

/// A Lit-scaled scalar constant on the given page (`translate = 0`).
pub fn scalar_on(
    page: u16,
    name: &str,
    ty: ScalarType,
    offset: usize,
    scale: f64,
    low: f64,
    high: f64,
) -> ConstantDef {
    ConstantDef {
        name: name.to_string(),
        page,
        offset,
        kind: ConstantKind::Scalar(ty),
        scale: Number::Lit(scale),
        translate: Number::Lit(0.0),
        units: String::new(),
        low: Number::Lit(low),
        high: Number::Lit(high),
        digits: 0,
    }
}

/// A Lit-scaled scalar constant on page 1.
pub fn scalar(
    name: &str,
    ty: ScalarType,
    offset: usize,
    scale: f64,
    low: f64,
    high: f64,
) -> ConstantDef {
    scalar_on(1, name, ty, offset, scale, low, high)
}

/// A two-page (numbers 1 and 2, `PAGE_SIZE` bytes each) tune around the
/// given constants.
pub fn tune(endianness: Endianness, constants: Vec<ConstantDef>) -> Tune {
    let pages = vec![
        PageDef {
            number: 1,
            size: PAGE_SIZE,
        },
        PageDef {
            number: 2,
            size: PAGE_SIZE,
        },
    ];
    Tune::new(Arc::new(Definition {
        comms: comms(endianness),
        pages,
        constants,
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
    }))
}

/// Load page 1 with `bytes` at offset 0, zero-padded to the page size.
pub fn load1(t: &mut Tune, bytes: &[u8]) {
    let mut page = vec![0u8; PAGE_SIZE];
    page[..bytes.len()].copy_from_slice(bytes);
    t.load_page(1, page);
}
