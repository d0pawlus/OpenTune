// SPDX-License-Identifier: GPL-3.0-or-later
//! Shared Definition/Tune builders for the `project` crate's `.msq` tests.
#![allow(dead_code)]

use std::sync::Arc;

use opentune_ini::{
    CommsSettings, ConstantDef, ConstantKind, Definition, Endianness, EnvelopeFormat, FrontPageDef,
    Number, PageDef, ScalarType, Shape,
};
use opentune_model::Tune;

pub const PAGE_SIZE: usize = 64;
pub const SIGNATURE: &str = "speeduino 202504-dev";

pub fn comms() -> CommsSettings {
    CommsSettings {
        signature: SIGNATURE.to_string(),
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

pub fn scalar(name: &str, ty: ScalarType, offset: usize, scale: f64, high: f64) -> ConstantDef {
    ConstantDef {
        name: name.to_string(),
        page: 1,
        offset,
        kind: ConstantKind::Scalar(ty),
        scale: Number::Lit(scale),
        translate: Number::Lit(0.0),
        units: String::new(),
        low: Number::Lit(0.0),
        high: Number::Lit(high),
        digits: 0,
    }
}

pub fn array_on(name: &str, offset: usize, rows: usize, cols: usize) -> ConstantDef {
    ConstantDef {
        name: name.to_string(),
        page: 1,
        offset,
        kind: ConstantKind::Array {
            elem: ScalarType::U08,
            shape: Shape { rows, cols },
        },
        scale: Number::Lit(1.0),
        translate: Number::Lit(0.0),
        units: String::new(),
        low: Number::Lit(0.0),
        high: Number::Lit(255.0),
        digits: 0,
    }
}

pub fn bits_on(name: &str, offset: usize, options: &[&str]) -> ConstantDef {
    ConstantDef {
        name: name.to_string(),
        page: 1,
        offset,
        kind: ConstantKind::Bits {
            storage: ScalarType::U08,
            bit_lo: 0,
            bit_hi: 1,
            options: options.iter().map(|s| s.to_string()).collect(),
        },
        scale: Number::Lit(1.0),
        translate: Number::Lit(0.0),
        units: String::new(),
        low: Number::Lit(0.0),
        high: Number::Lit(3.0),
        digits: 0,
    }
}

pub fn tune(constants: Vec<ConstantDef>) -> Tune {
    let pages = vec![PageDef {
        number: 1,
        size: PAGE_SIZE,
    }];
    Tune::new(Arc::new(Definition {
        comms: comms(),
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
        ve_analyze: None,
    }))
}
