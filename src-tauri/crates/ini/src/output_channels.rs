// SPDX-License-Identifier: GPL-3.0-or-later
//! M3 `[OutputChannels]` types — realtime frame channel definitions.
//!
//! Each entry describes how to decode one named value out of the raw
//! `read_output_channels` byte block (the "och block") the ECU streams during
//! realtime polling. Real parsing (Task 2) reads these off the wire; this
//! module only freezes the shape so the `realtime` crate's `decode_frame` seam
//! can be pinned before the parser exists.

use crate::ScalarType;

/// One `[OutputChannels]` entry. Realtime frames decode against these.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub enum OutputChannelDef {
    /// `map = scalar, U16, 4, "kpa", 1.000, 0.000` — offset into the och block.
    /// physical = raw*scale + translate. No min/max/digits (unlike ConstantDef).
    Scalar {
        name: String,
        kind: ScalarType,
        offset: usize,
        units: String,
        scale: f64,
        translate: f64,
    },
    /// `running = bits, U08, 2, [0:0]` — flag/enum over a byte already in the block.
    Bits {
        name: String,
        storage: ScalarType,
        offset: usize,
        bit_lo: u8,
        bit_hi: u8,
    },
    /// `coolant = { coolantRaw - 40 }`, `throttle = { tps }, "%"`. Expression is an
    /// opaque string (ported hyper-tuner shape), evaluated lazily by `realtime`.
    Computed {
        name: String,
        expr: String,
        /// "" when the entry declares no trailing units.
        units: String,
    },
}

impl OutputChannelDef {
    /// The channel's name, regardless of variant.
    pub fn name(&self) -> &str {
        match self {
            OutputChannelDef::Scalar { name, .. } => name,
            OutputChannelDef::Bits { name, .. } => name,
            OutputChannelDef::Computed { name, .. } => name,
        }
    }
}
