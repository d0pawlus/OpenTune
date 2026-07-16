// SPDX-License-Identifier: GPL-3.0-or-later
//! The real per-channel och-block decoder (M3 Task 6, sub-step 6.3).
//!
//! Two passes over [`Definition::output_channels`]:
//!
//! 1. Every `Scalar`/`Bits` channel decodes straight from the raw block
//!    (raw → physical via `scale`/`translate`, per [`ScalarType`] width and
//!    the INI-declared endianness). Every slice is guarded against the
//!    block length — a short buffer degrades that one channel to a
//!    diagnostic, never the frame.
//! 2. Every `Computed` channel evaluates its `{ expr }` via
//!    [`opentune_ini::eval`] with a lookup over the already-decoded values
//!    — in file order, so chains like `coolant → chained` resolve. Any
//!    [`ExprError`](opentune_ini::ExprError) (unknown variable, unsupported
//!    function, …) degrades that channel to a diagnostic.
//!
//! **Fail-open per channel** is the global M3 constraint: one bad
//! channel/expression never blanks the frame.
//!
//! The raw-scalar readers are written fresh here rather than importing
//! `opentune-model`'s codec: this crate's frozen seam depends on
//! `opentune-ini` only (Task 0.4), and the ~30 duplicated lines are the
//! cost of keeping `realtime` decode-only.

use std::collections::HashMap;

use opentune_ini::{Definition, Endianness, OutputChannelDef, ScalarType};

use crate::{ChannelValue, RealtimeFrame};

/// Decode one raw och block against the definition's channels into physical
/// values. See the module doc for the two-pass, fail-open contract.
pub fn decode_frame(def: &Definition, block: &[u8]) -> RealtimeFrame {
    let endian = def.comms.endianness;
    let mut channels = Vec::with_capacity(def.output_channels.len());
    let mut diagnostics = Vec::new();
    let mut values: HashMap<&str, f64> = HashMap::new();

    // Pass 1: raw scalar/bits channels, straight from the block.
    for ch in &def.output_channels {
        let decoded = match ch {
            OutputChannelDef::Scalar {
                name,
                kind,
                offset,
                scale,
                translate,
                ..
            } => decode_scalar(block, *kind, *offset, endian)
                // TunerStudio formula: physical = (raw + translate) * scale.
                .map(|raw| (name.as_str(), (raw + translate) * scale)),
            OutputChannelDef::Bits {
                name,
                storage,
                offset,
                bit_lo,
                bit_hi,
            } => decode_bits(block, *storage, *offset, *bit_lo, *bit_hi, endian)
                .map(|v| (name.as_str(), v)),
            OutputChannelDef::Computed { .. } => continue, // pass 2
        };
        match decoded {
            Some((name, value)) => {
                values.insert(name, value);
                channels.push(ChannelValue {
                    name: name.to_string(),
                    value,
                });
            }
            None => diagnostics.push(ch.name().to_string()),
        }
    }

    // Pass 2: computed channels, in file order so chains resolve.
    for ch in &def.output_channels {
        let OutputChannelDef::Computed { name, expr, .. } = ch else {
            continue;
        };
        let lookup = |var: &str| values.get(var).copied();
        // Safety note: `opentune_ini::eval` is the project's *sandboxed*
        // arithmetic-expression evaluator (fixed grammar, no code execution,
        // no I/O — see `crates/ini/src/expr.rs`), not a general `eval`.
        match opentune_ini::eval(expr, &lookup) {
            Ok(value) => {
                values.insert(name.as_str(), value);
                channels.push(ChannelValue {
                    name: name.clone(),
                    value,
                });
            }
            Err(_) => diagnostics.push(name.clone()),
        }
    }

    RealtimeFrame {
        channels,
        diagnostics,
    }
}

/// Byte width of a scalar storage type.
fn width(ty: ScalarType) -> usize {
    match ty {
        ScalarType::U08 | ScalarType::S08 => 1,
        ScalarType::U16 | ScalarType::S16 => 2,
        ScalarType::U32 | ScalarType::S32 | ScalarType::F32 => 4,
    }
}

/// The channel's byte region, or `None` when it falls outside the block
/// (short buffer ⇒ fail-open diagnostic upstream).
fn region(block: &[u8], offset: usize, len: usize) -> Option<&[u8]> {
    block.get(offset..offset.checked_add(len)?)
}

/// Decode one raw scalar at `offset`, or `None` on a short buffer.
fn decode_scalar(block: &[u8], ty: ScalarType, offset: usize, endian: Endianness) -> Option<f64> {
    let b = region(block, offset, width(ty))?;
    let le = endian == Endianness::Little;
    Some(match ty {
        ScalarType::U08 => f64::from(b[0]),
        ScalarType::S08 => f64::from(b[0] as i8),
        ScalarType::U16 => f64::from(u16::from_bytes(b, le)),
        ScalarType::S16 => f64::from(u16::from_bytes(b, le) as i16),
        ScalarType::U32 => f64::from(u32::from_bytes(b, le)),
        ScalarType::S32 => f64::from(u32::from_bytes(b, le) as i32),
        ScalarType::F32 => f64::from(f32::from_bits(u32::from_bytes(b, le))),
    })
}

/// Decode a `Bits` channel to its selected field value (e.g. `[0:0]` → 0/1),
/// or `None` on a short buffer or a bit range that does not fit the storage.
fn decode_bits(
    block: &[u8],
    storage: ScalarType,
    offset: usize,
    bit_lo: u8,
    bit_hi: u8,
    endian: Endianness,
) -> Option<f64> {
    let b = region(block, offset, width(storage))?;
    let storage_bits = width(storage) as u8 * 8;
    if bit_lo > bit_hi || bit_hi >= storage_bits {
        return None;
    }
    let le = endian == Endianness::Little;
    let pattern: u64 = match width(storage) {
        1 => u64::from(b[0]),
        2 => u64::from(u16::from_bytes(b, le)),
        _ => u64::from(u32::from_bytes(b, le)),
    };
    let mask = (1u64 << (bit_hi - bit_lo + 1)) - 1;
    Some(((pattern >> bit_lo) & mask) as f64)
}

/// Endianness-dispatching `from_le_bytes`/`from_be_bytes` over a checked
/// slice (the caller guarantees `b.len() == size_of::<Self>()`).
trait FromBytes: Sized {
    fn from_bytes(b: &[u8], little: bool) -> Self;
}

impl FromBytes for u16 {
    fn from_bytes(b: &[u8], little: bool) -> Self {
        let arr = [b[0], b[1]];
        if little {
            u16::from_le_bytes(arr)
        } else {
            u16::from_be_bytes(arr)
        }
    }
}

impl FromBytes for u32 {
    fn from_bytes(b: &[u8], little: bool) -> Self {
        let arr = [b[0], b[1], b[2], b[3]];
        if little {
            u32::from_le_bytes(arr)
        } else {
            u32::from_be_bytes(arr)
        }
    }
}
