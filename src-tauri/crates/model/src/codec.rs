// SPDX-License-Identifier: GPL-3.0-or-later
//! Pure codec for [`Tune`](crate::Tune) — raw scalar decode/encode, `Bits`
//! bit-pattern access, and the per-kind [`Value`] conversions, all
//! endianness-aware.
//!
//! Split out of `tune.rs` for file-size cohesion (the plan's `codec.rs`
//! split): everything here is a pure function over byte slices and resolved
//! numbers, with no `Tune` state. `tune.rs` owns orchestration, `Number`
//! resolution, and dirty/undo bookkeeping.

use opentune_ini::{ConstantKind, Endianness, ScalarType, Shape};

use crate::tune::ModelError;
use crate::value::Value;

/// A constant's `scale`/`translate`/`low`/`high` resolved to concrete
/// numbers for a write.
pub(crate) struct Scaling {
    pub(crate) scale: f64,
    pub(crate) translate: f64,
    pub(crate) low: f64,
    pub(crate) high: f64,
}

/// Short human label for a [`ConstantKind`], for `TypeMismatch` messages.
pub(crate) fn kind_label(kind: &ConstantKind) -> &'static str {
    match kind {
        ConstantKind::Scalar(_) => "scalar",
        ConstantKind::Array { .. } => "array",
        ConstantKind::Bits { .. } => "bits",
        ConstantKind::Text { .. } => "string",
    }
}

/// Byte width of a scalar storage type.
pub(crate) fn width(ty: ScalarType) -> usize {
    match ty {
        ScalarType::U08 | ScalarType::S08 => 1,
        ScalarType::U16 | ScalarType::S16 => 2,
        ScalarType::U32 | ScalarType::S32 | ScalarType::F32 => 4,
    }
}

/// The `(min, max)` raw values a scalar storage type can hold. Backs the
/// `getChannelMin/MaxByOffset` builtins' encodable-range approximation.
pub(crate) fn raw_range(ty: ScalarType) -> (f64, f64) {
    match ty {
        ScalarType::U08 => (0.0, f64::from(u8::MAX)),
        ScalarType::S08 => (f64::from(i8::MIN), f64::from(i8::MAX)),
        ScalarType::U16 => (0.0, f64::from(u16::MAX)),
        ScalarType::S16 => (f64::from(i16::MIN), f64::from(i16::MAX)),
        ScalarType::U32 => (0.0, f64::from(u32::MAX)),
        ScalarType::S32 => (f64::from(i32::MIN), f64::from(i32::MAX)),
        ScalarType::F32 => (f64::from(f32::MIN), f64::from(f32::MAX)),
    }
}

fn arr2(b: &[u8]) -> [u8; 2] {
    [b[0], b[1]]
}

fn arr4(b: &[u8]) -> [u8; 4] {
    [b[0], b[1], b[2], b[3]]
}

/// Decode one raw scalar from the first [`width`]`(ty)` bytes of `bytes`.
pub(crate) fn read_raw(bytes: &[u8], ty: ScalarType, endian: Endianness) -> f64 {
    use Endianness::{Big, Little};
    use ScalarType as T;
    match (ty, endian) {
        (T::U08, _) => f64::from(bytes[0]),
        (T::S08, _) => f64::from(bytes[0] as i8),
        (T::U16, Little) => f64::from(u16::from_le_bytes(arr2(bytes))),
        (T::U16, Big) => f64::from(u16::from_be_bytes(arr2(bytes))),
        (T::S16, Little) => f64::from(i16::from_le_bytes(arr2(bytes))),
        (T::S16, Big) => f64::from(i16::from_be_bytes(arr2(bytes))),
        (T::U32, Little) => f64::from(u32::from_le_bytes(arr4(bytes))),
        (T::U32, Big) => f64::from(u32::from_be_bytes(arr4(bytes))),
        (T::S32, Little) => f64::from(i32::from_le_bytes(arr4(bytes))),
        (T::S32, Big) => f64::from(i32::from_be_bytes(arr4(bytes))),
        (T::F32, Little) => f64::from(f32::from_le_bytes(arr4(bytes))),
        (T::F32, Big) => f64::from(f32::from_be_bytes(arr4(bytes))),
    }
}

fn fits(r: f64, min: f64, max: f64) -> Option<f64> {
    (min..=max).contains(&r).then_some(r)
}

/// Encode an inverse-scaled raw value into storage bytes.
///
/// Integer types round **half away from zero** (`f64::round`, so `4.5 -> 5`
/// and `-4.5 -> -5`); `F32` is stored unrounded (rounding would destroy
/// legitimate fractional raw values). Returns `None` when `raw` is
/// non-finite (including the `scale = 0` division blow-up) or the rounded
/// value does not fit the storage type — the caller surfaces both as
/// [`ModelError::OutOfRange`].
pub(crate) fn encode_raw(raw: f64, ty: ScalarType, endian: Endianness) -> Option<Vec<u8>> {
    use Endianness::{Big, Little};
    use ScalarType as T;
    if !raw.is_finite() {
        return None;
    }
    if ty == T::F32 {
        let v = raw as f32;
        return Some(match endian {
            Little => v.to_le_bytes().to_vec(),
            Big => v.to_be_bytes().to_vec(),
        });
    }
    let r = raw.round();
    Some(match (ty, endian) {
        (T::U08, _) => vec![fits(r, 0.0, f64::from(u8::MAX))? as u8],
        (T::S08, _) => vec![(fits(r, f64::from(i8::MIN), f64::from(i8::MAX))? as i8) as u8],
        (T::U16, Little) => (fits(r, 0.0, f64::from(u16::MAX))? as u16)
            .to_le_bytes()
            .to_vec(),
        (T::U16, Big) => (fits(r, 0.0, f64::from(u16::MAX))? as u16)
            .to_be_bytes()
            .to_vec(),
        (T::S16, Little) => (fits(r, f64::from(i16::MIN), f64::from(i16::MAX))? as i16)
            .to_le_bytes()
            .to_vec(),
        (T::S16, Big) => (fits(r, f64::from(i16::MIN), f64::from(i16::MAX))? as i16)
            .to_be_bytes()
            .to_vec(),
        (T::U32, Little) => (fits(r, 0.0, f64::from(u32::MAX))? as u32)
            .to_le_bytes()
            .to_vec(),
        (T::U32, Big) => (fits(r, 0.0, f64::from(u32::MAX))? as u32)
            .to_be_bytes()
            .to_vec(),
        (T::S32, Little) => (fits(r, f64::from(i32::MIN), f64::from(i32::MAX))? as i32)
            .to_le_bytes()
            .to_vec(),
        (T::S32, Big) => (fits(r, f64::from(i32::MIN), f64::from(i32::MAX))? as i32)
            .to_be_bytes()
            .to_vec(),
        (T::F32, _) => unreachable!("handled above"),
    })
}

/// Read the unsigned bit pattern of a `Bits` storage location. Signed
/// storage types are treated as their unsigned bit pattern.
pub(crate) fn read_pattern(bytes: &[u8], ty: ScalarType, endian: Endianness) -> u64 {
    match width(ty) {
        1 => u64::from(bytes[0]),
        2 => match endian {
            Endianness::Little => u64::from(u16::from_le_bytes(arr2(bytes))),
            Endianness::Big => u64::from(u16::from_be_bytes(arr2(bytes))),
        },
        _ => match endian {
            Endianness::Little => u64::from(u32::from_le_bytes(arr4(bytes))),
            Endianness::Big => u64::from(u32::from_be_bytes(arr4(bytes))),
        },
    }
}

/// Encode an unsigned bit pattern back into `Bits` storage bytes.
pub(crate) fn pattern_bytes(pattern: u64, ty: ScalarType, endian: Endianness) -> Vec<u8> {
    match width(ty) {
        1 => vec![pattern as u8],
        2 => match endian {
            Endianness::Little => (pattern as u16).to_le_bytes().to_vec(),
            Endianness::Big => (pattern as u16).to_be_bytes().to_vec(),
        },
        _ => match endian {
            Endianness::Little => (pattern as u32).to_le_bytes().to_vec(),
            Endianness::Big => (pattern as u32).to_be_bytes().to_vec(),
        },
    }
}

/// Validate a `Bits` bit range against its storage width and return the
/// **unshifted** field mask (e.g. `[4:7]` -> `0xF`).
///
/// A malformed range (inverted, or extending past the storage type) is a
/// definition inconsistency, surfaced as [`ModelError::TypeMismatch`]
/// rather than a panic.
pub(crate) fn bits_mask(
    name: &str,
    storage: ScalarType,
    bit_lo: u8,
    bit_hi: u8,
) -> Result<u64, ModelError> {
    let storage_bits = width(storage) as u8 * 8;
    if bit_lo > bit_hi || bit_hi >= storage_bits {
        return Err(ModelError::TypeMismatch(format!(
            "`{name}`: bit range [{bit_lo}:{bit_hi}] does not fit {storage_bits}-bit storage"
        )));
    }
    Ok((1u64 << (bit_hi - bit_lo + 1)) - 1)
}

/// Decode a scalar region to its physical value, per TunerStudio's
/// documented formula: `userValue = (msValue + translate) * scale`. NOT
/// `raw * scale + translate` — the two agree only when `translate == 0`
/// or `scale == 1` (see `tests/scaling.rs`).
pub(crate) fn decode_scalar(
    region: &[u8],
    ty: ScalarType,
    endian: Endianness,
    scale: f64,
    translate: f64,
) -> Value {
    Value::Scalar((read_raw(region, ty, endian) + translate) * scale)
}

/// Decode an array region element-wise (row-major) to physical values.
pub(crate) fn decode_array(
    region: &[u8],
    elem: ScalarType,
    endian: Endianness,
    scale: f64,
    translate: f64,
) -> Value {
    let w = width(elem);
    Value::Array(
        region
            .chunks_exact(w)
            .map(|chunk| (read_raw(chunk, elem, endian) + translate) * scale)
            .collect(),
    )
}

/// Decode a `Bits` region to the selected option index.
pub(crate) fn decode_bits(
    name: &str,
    region: &[u8],
    storage: ScalarType,
    bit_lo: u8,
    bit_hi: u8,
    endian: Endianness,
) -> Result<Value, ModelError> {
    let mask = bits_mask(name, storage, bit_lo, bit_hi)?;
    let pattern = read_pattern(region, storage, endian);
    Ok(Value::Enum(((pattern >> bit_lo) & mask) as u32))
}

/// Decode a text region: the bytes up to the first NUL (or the whole
/// region), lossily decoded as UTF-8.
pub(crate) fn decode_text(region: &[u8]) -> Value {
    let end = region.iter().position(|&b| b == 0).unwrap_or(region.len());
    Value::Text(String::from_utf8_lossy(&region[..end]).into_owned())
}

/// Range-check a physical value against the inclusive `[low, high]` bounds
/// and encode its inverse-scaled raw bytes, per TunerStudio's documented
/// formula: `msValue = userValue / scale - translate`.
pub(crate) fn encode_scalar(
    name: &str,
    s: &Scaling,
    x: f64,
    ty: ScalarType,
    endian: Endianness,
) -> Result<Vec<u8>, ModelError> {
    let out_of_range = || ModelError::OutOfRange {
        name: name.to_string(),
        value: x,
    };
    // Normalize an inverted low/high declaration (real MS3 typo fallout:
    // `psInitValue` shifts to low=1, high=0) — the declared ORDER must not
    // reject every value.
    let (low, high) = (s.low.min(s.high), s.low.max(s.high));
    if !x.is_finite() || x < low || x > high {
        return Err(out_of_range());
    }
    encode_raw(x / s.scale - s.translate, ty, endian).ok_or_else(out_of_range)
}

/// Encode an array element-wise; the element count must match `shape`
/// (`TypeMismatch` otherwise) and every element is range-checked.
pub(crate) fn encode_array(
    name: &str,
    s: &Scaling,
    xs: &[f64],
    elem: ScalarType,
    shape: Shape,
    endian: Endianness,
) -> Result<Vec<u8>, ModelError> {
    let expected = shape.rows * shape.cols;
    if xs.len() != expected {
        return Err(ModelError::TypeMismatch(format!(
            "`{name}`: expected {expected} elements, got {}",
            xs.len()
        )));
    }
    let mut bytes = Vec::with_capacity(expected * width(elem));
    for x in xs {
        bytes.extend(encode_scalar(name, s, *x, elem, endian)?);
    }
    Ok(bytes)
}

/// Mask an option index into the `current` storage bytes, preserving
/// neighboring bits. An index beyond the options list (when options are
/// declared) or beyond the bit-field capacity is `OutOfRange`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn encode_bits(
    name: &str,
    current: &[u8],
    storage: ScalarType,
    bit_lo: u8,
    bit_hi: u8,
    options: &[String],
    index: u32,
    endian: Endianness,
) -> Result<Vec<u8>, ModelError> {
    let mask = bits_mask(name, storage, bit_lo, bit_hi)?;
    let beyond_capacity = u64::from(index) > mask;
    let beyond_options = !options.is_empty() && (index as usize) >= options.len();
    if beyond_capacity || beyond_options {
        return Err(ModelError::OutOfRange {
            name: name.to_string(),
            value: f64::from(index),
        });
    }
    let pattern =
        (read_pattern(current, storage, endian) & !(mask << bit_lo)) | (u64::from(index) << bit_lo);
    Ok(pattern_bytes(pattern, storage, endian))
}

/// Encode text as fixed-length bytes, zero-padding shorter values.
///
/// Text longer than the declared length is a `TypeMismatch` — a structural
/// mismatch with the field, not a numeric range violation (and
/// `OutOfRange` carries an `f64` that cannot represent a string anyway).
pub(crate) fn encode_text(name: &str, text: &str, len: usize) -> Result<Vec<u8>, ModelError> {
    if text.len() > len {
        return Err(ModelError::TypeMismatch(format!(
            "`{name}`: text is {} bytes but the field holds {len}",
            text.len()
        )));
    }
    let mut bytes = vec![0u8; len];
    bytes[..text.len()].copy_from_slice(text.as_bytes());
    Ok(bytes)
}
