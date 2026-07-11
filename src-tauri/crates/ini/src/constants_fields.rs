// SPDX-License-Identifier: GPL-3.0-or-later
//! Field-level tokenizing and per-class parsing for one `[Constants]` /
//! `[PcVariables]` line — split out of `constants_parser.rs` to keep each
//! file focused (see sub-step 1.4).
//!
//! Port source (ADR-0006): [`hyper-tuner/ini`](https://github.com/hyper-tuner/ini)
//! (MIT) — `parseConstAndVar` establishes the field order for `scalar`/
//! `array`/`bits` (`name = class, type, offset, [shape], units, scale,
//! transform, min, max, digits`). Field renames honored: hyper-tuner's
//! `transform` → our [`ConstantDef::translate`]; its `min`/`max` → our
//! [`ConstantDef::low`]/[`ConstantDef::high`].
//!
//! Gaps filled (written fresh — absent from hyper-tuner):
//! - `lastOffset` resolution via a running per-page byte counter.
//! - String-type constants reachable from `[Constants]` (hyper-tuner's
//!   `parseConstants` switch has no `case 'string'` — it silently drops
//!   them). Structural reference: `adbancroft/TunerStudioIniParser`'s
//!   `StringVariable`/`ts_ini.lark` grammar (LGPLv3) confirmed the field
//!   order `name = string, ENCODING, offset, LENGTH`. Its `type_factory.py`
//!   has an unrelated `F32`→`U08` `DataType.type_name` typo — irrelevant
//!   to string handling, and this port maps `ScalarType`/`Text` freshly
//!   rather than copying that table, so the typo cannot propagate here.

use crate::{ConstantDef, ConstantKind, IniError, Number, ScalarType, Shape};

/// Per-page running `lastOffset` state.
///
/// An unrecognised constant `class` cannot be sized, so the caller
/// (`constants_parser.rs`) cannot know how many bytes it occupies and must
/// stop trusting the running counter for that page — [`Self::Poisoned`]
/// records that. Resolving `lastOffset` while poisoned is refused (see
/// [`resolve_offset`]) rather than silently returning the stale
/// pre-poison value, which would desync every later `lastOffset` constant
/// onto a wrong-but-plausible offset. An explicit numeric offset is
/// unaffected by the poison and, on success, re-anchors the page back to
/// [`Self::Known`].
#[derive(Debug, Clone, Copy)]
pub(crate) enum OffsetCounter {
    Known(usize),
    Poisoned,
}

impl OffsetCounter {
    pub(crate) fn zero() -> Self {
        OffsetCounter::Known(0)
    }
}

/// The result of resolving one constant's offset field.
enum OffsetResolution {
    Value(usize),
    /// The field was `lastOffset`, but the page's running counter is
    /// [`OffsetCounter::Poisoned`] — refuse to resolve rather than return a
    /// desynced value.
    Poisoned,
}

/// The outcome of parsing one recognised-class constant line.
pub(crate) enum FieldOutcome {
    Def(ConstantDef),
    /// The class was recognised but its offset field was `lastOffset` on a
    /// poisoned page; the caller records a diagnostic and skips the
    /// constant instead of adding it.
    PoisonedOffset,
}

/// Split a constant's value tail into comma-separated fields, respecting
/// double-quoted strings and `{ ... }` expressions (both of which may
/// themselves contain commas, e.g. `{ a, b }` is not valid here but a
/// units string never legitimately contains an unescaped `,` either way —
/// we still guard both delimiters defensively).
pub(crate) fn split_fields(value: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    let mut brace_depth = 0u32;

    for ch in value.chars() {
        match ch {
            '"' => {
                in_quote = !in_quote;
                current.push(ch);
            }
            '{' if !in_quote => {
                brace_depth += 1;
                current.push(ch);
            }
            '}' if !in_quote => {
                brace_depth = brace_depth.saturating_sub(1);
                current.push(ch);
            }
            ',' if !in_quote && brace_depth == 0 => {
                fields.push(current.trim().to_string());
                current = String::new();
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() || !fields.is_empty() {
        fields.push(current.trim().to_string());
    }
    fields
}

pub(crate) fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

/// Parse a number-or-expression field: `{ expr }` becomes
/// [`Number::Expr`] with braces stripped and whitespace trimmed; anything
/// else is parsed as a literal float.
pub(crate) fn parse_number(field: &str) -> Number {
    let trimmed = field.trim();
    if let Some(inner) = trimmed.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
        return Number::Expr(inner.trim().to_string());
    }
    match trimmed.parse::<f64>() {
        Ok(n) => Number::Lit(n),
        Err(_) => Number::Expr(trimmed.to_string()),
    }
}

fn number_or_default(fields: &[String], index: usize, default: f64) -> Number {
    fields
        .get(index)
        .filter(|field| !field.trim().is_empty())
        .map(|field| parse_number(field))
        .unwrap_or(Number::Lit(default))
}

pub(crate) fn parse_scalar_type(s: &str) -> Option<ScalarType> {
    match s.trim() {
        "U08" => Some(ScalarType::U08),
        "S08" => Some(ScalarType::S08),
        "U16" => Some(ScalarType::U16),
        "S16" => Some(ScalarType::S16),
        "U32" => Some(ScalarType::U32),
        "S32" => Some(ScalarType::S32),
        "F32" => Some(ScalarType::F32),
        _ => None,
    }
}

/// Byte width of a scalar type, used to advance the `lastOffset` counter.
pub(crate) fn scalar_width(t: ScalarType) -> usize {
    match t {
        ScalarType::U08 | ScalarType::S08 => 1,
        ScalarType::U16 | ScalarType::S16 => 2,
        ScalarType::U32 | ScalarType::S32 | ScalarType::F32 => 4,
    }
}

pub(crate) fn array_byte_size(name: &str, elem: ScalarType, shape: Shape) -> crate::Result<usize> {
    shape
        .rows
        .checked_mul(shape.cols)
        .and_then(|elements| elements.checked_mul(scalar_width(elem)))
        .ok_or_else(|| invalid(name, "array shape/byte size overflows platform limits"))
}

fn checked_running_offset(name: &str, offset: usize, size: usize) -> crate::Result<OffsetCounter> {
    offset
        .checked_add(size)
        .map(OffsetCounter::Known)
        .ok_or_else(|| invalid(name, "offset + byte size overflows platform limits"))
}

/// Parse the `[RxC]` / `[N]` array shape syntax. `[N]` is a 1-D array of N
/// elements (`Shape { rows: N, cols: 1 }`); `[RxC]` is a 2-D table with R
/// rows and C columns, per the canonical Speeduino doc comment ("[2x4]
/// defines a table with eight values (two rows and four columns)").
fn parse_shape(s: &str) -> Option<Shape> {
    let inner = s.trim().strip_prefix('[')?.strip_suffix(']')?.trim();
    if let Some((rows, cols)) = inner.split_once(['x', 'X']) {
        Some(Shape {
            rows: rows.trim().parse().ok()?,
            cols: cols.trim().parse().ok()?,
        })
    } else {
        Some(Shape {
            rows: inner.parse().ok()?,
            cols: 1,
        })
    }
}

/// Parse the `[lo:hi]` bit-range syntax (both bounds inclusive), per the
/// real Speeduino data verified against `mapSample`/`nCylinders` (a
/// 2-value field spans `[0:1]`; a 16-value field spans `[4:7]`).
pub(crate) fn parse_bit_range(s: &str) -> Option<(u8, u8)> {
    let inner = s.trim().strip_prefix('[')?.strip_suffix(']')?.trim();
    let (lo, hi) = inner.split_once(':')?;
    Some((lo.trim().parse().ok()?, hi.trim().parse().ok()?))
}

/// Resolve an offset field: either a literal integer, or the `lastOffset`
/// keyword, which resolves to the running per-page byte counter — unless
/// that counter is [`OffsetCounter::Poisoned`], in which case resolution is
/// refused (see [`OffsetCounter`]'s doc comment). A literal integer is
/// always honored regardless of poison.
fn resolve_offset(field: &str, running: &OffsetCounter) -> Option<OffsetResolution> {
    if field.trim() == "lastOffset" {
        match running {
            OffsetCounter::Known(v) => Some(OffsetResolution::Value(*v)),
            OffsetCounter::Poisoned => Some(OffsetResolution::Poisoned),
        }
    } else {
        field
            .trim()
            .parse::<usize>()
            .ok()
            .map(OffsetResolution::Value)
    }
}

fn invalid(name: &str, detail: &str) -> IniError {
    IniError::InvalidValue {
        key: name.to_string(),
        detail: detail.to_string(),
    }
}

/// The result of parsing one `[Constants]`/`[PcVariables]` line, for the
/// caller (`constants_parser.rs`) to turn into either a stored
/// [`ConstantDef`] or a `Diagnostic`.
pub(crate) enum ConstantLineResult {
    Def(ConstantDef),
    /// The `class` token wasn't recognised.
    UnknownClass,
    /// The class was recognised, but its `lastOffset` field couldn't be
    /// trusted because an earlier unrecognised constant poisoned the
    /// page's running offset (see [`OffsetCounter`]).
    PoisonedOffset,
}

/// Parse one `name = class, ...` constant line.
///
/// `page` is `None` for `[PcVariables]` entries (no offset field in that
/// section's grammar). `running_offset` is the per-page `lastOffset`
/// counter; it is advanced past the field's byte width on success.
///
/// Returns [`ConstantLineResult::UnknownClass`] for an unrecognised `class`
/// so the caller can degrade gracefully with a `Diagnostic` instead of
/// failing the whole parse.
pub(crate) fn parse_constant_line(
    name: &str,
    value: &str,
    page: Option<u16>,
    running_offset: &mut OffsetCounter,
) -> crate::Result<ConstantLineResult> {
    let fields = split_fields(value);
    let Some(class) = fields.first().map(String::as_str) else {
        return Ok(ConstantLineResult::UnknownClass);
    };

    let outcome = match (class, page) {
        ("scalar", Some(p)) => parse_scalar(name, &fields, p, running_offset)?,
        ("scalar", None) => FieldOutcome::Def(parse_scalar_no_offset(name, &fields)?),
        ("array", Some(p)) => parse_array(name, &fields, p, running_offset)?,
        ("bits", Some(p)) => parse_bits(name, &fields, p, running_offset)?,
        ("string", Some(p)) => parse_string(name, &fields, p, running_offset)?,
        _ => return Ok(ConstantLineResult::UnknownClass),
    };

    Ok(match outcome {
        FieldOutcome::Def(def) => ConstantLineResult::Def(def),
        FieldOutcome::PoisonedOffset => ConstantLineResult::PoisonedOffset,
    })
}

/// `name = scalar, TYPE, offset, units, scale, translate, low, high, digits`
fn parse_scalar(
    name: &str,
    fields: &[String],
    page: u16,
    running_offset: &mut OffsetCounter,
) -> crate::Result<FieldOutcome> {
    let scalar_type = fields
        .get(1)
        .and_then(|s| parse_scalar_type(s))
        .ok_or_else(|| invalid(name, "unrecognised scalar type"))?;
    let offset = match fields
        .get(2)
        .and_then(|s| resolve_offset(s, running_offset))
    {
        Some(OffsetResolution::Value(v)) => v,
        Some(OffsetResolution::Poisoned) => return Ok(FieldOutcome::PoisonedOffset),
        None => return Err(invalid(name, "unparseable offset")),
    };
    let units = fields.get(3).map(|s| unquote(s)).unwrap_or_default();
    let scale = number_or_default(fields, 4, 1.0);
    let translate = number_or_default(fields, 5, 0.0);
    let low = number_or_default(fields, 6, 0.0);
    let high = number_or_default(fields, 7, 0.0);
    let digits = fields
        .get(8)
        .and_then(|s| s.trim().parse::<u8>().ok())
        .unwrap_or(0);

    *running_offset = checked_running_offset(name, offset, scalar_width(scalar_type))?;

    Ok(FieldOutcome::Def(ConstantDef {
        name: name.to_string(),
        page,
        offset,
        kind: ConstantKind::Scalar(scalar_type),
        scale,
        translate,
        units,
        low,
        high,
        digits,
    }))
}

/// `[PcVariables]` scalar: `name = scalar, TYPE, units, scale, translate, low, high, digits`
/// (no offset field — host-only variables aren't stored in ECU memory).
fn parse_scalar_no_offset(name: &str, fields: &[String]) -> crate::Result<ConstantDef> {
    let scalar_type = fields
        .get(1)
        .and_then(|s| parse_scalar_type(s))
        .ok_or_else(|| invalid(name, "unrecognised scalar type"))?;
    let units = fields.get(2).map(|s| unquote(s)).unwrap_or_default();
    let scale = number_or_default(fields, 3, 1.0);
    let translate = number_or_default(fields, 4, 0.0);
    let low = number_or_default(fields, 5, 0.0);
    let high = number_or_default(fields, 6, 0.0);
    let digits = fields
        .get(7)
        .and_then(|s| s.trim().parse::<u8>().ok())
        .unwrap_or(0);

    Ok(ConstantDef {
        name: name.to_string(),
        page: 0,
        offset: 0,
        kind: ConstantKind::Scalar(scalar_type),
        scale,
        translate,
        units,
        low,
        high,
        digits,
    })
}

/// `name = array, TYPE, offset, [shape], units, scale, translate, low, high, digits`
fn parse_array(
    name: &str,
    fields: &[String],
    page: u16,
    running_offset: &mut OffsetCounter,
) -> crate::Result<FieldOutcome> {
    let elem = fields
        .get(1)
        .and_then(|s| parse_scalar_type(s))
        .ok_or_else(|| invalid(name, "unrecognised array element type"))?;
    let offset = match fields
        .get(2)
        .and_then(|s| resolve_offset(s, running_offset))
    {
        Some(OffsetResolution::Value(v)) => v,
        Some(OffsetResolution::Poisoned) => return Ok(FieldOutcome::PoisonedOffset),
        None => return Err(invalid(name, "unparseable offset")),
    };
    let shape = fields
        .get(3)
        .and_then(|s| parse_shape(s))
        .ok_or_else(|| invalid(name, "unparseable array shape"))?;
    let units = fields.get(4).map(|s| unquote(s)).unwrap_or_default();
    let scale = number_or_default(fields, 5, 1.0);
    let translate = number_or_default(fields, 6, 0.0);
    let low = number_or_default(fields, 7, 0.0);
    let high = number_or_default(fields, 8, 0.0);
    let digits = fields
        .get(9)
        .and_then(|s| s.trim().parse::<u8>().ok())
        .unwrap_or(0);

    let byte_size = array_byte_size(name, elem, shape)?;
    *running_offset = checked_running_offset(name, offset, byte_size)?;

    Ok(FieldOutcome::Def(ConstantDef {
        name: name.to_string(),
        page,
        offset,
        kind: ConstantKind::Array { elem, shape },
        scale,
        translate,
        units,
        low,
        high,
        digits,
    }))
}

/// `name = bits, TYPE, offset, [lo:hi], "option0", "option1", ...`
///
/// Bits have no scale/translate/units/low/high/digits tail in the INI —
/// they select from a fixed option list instead. We store neutral
/// defaults for the shared numeric fields since [`ConstantDef`] always
/// carries them.
fn parse_bits(
    name: &str,
    fields: &[String],
    page: u16,
    running_offset: &mut OffsetCounter,
) -> crate::Result<FieldOutcome> {
    let storage = fields
        .get(1)
        .and_then(|s| parse_scalar_type(s))
        .ok_or_else(|| invalid(name, "unrecognised bits storage type"))?;
    let offset = match fields
        .get(2)
        .and_then(|s| resolve_offset(s, running_offset))
    {
        Some(OffsetResolution::Value(v)) => v,
        Some(OffsetResolution::Poisoned) => return Ok(FieldOutcome::PoisonedOffset),
        None => return Err(invalid(name, "unparseable offset")),
    };
    let (bit_lo, bit_hi) = fields
        .get(3)
        .and_then(|s| parse_bit_range(s))
        .ok_or_else(|| invalid(name, "unparseable bit range"))?;
    let options: Vec<String> = fields.iter().skip(4).map(|s| unquote(s)).collect();

    *running_offset = checked_running_offset(name, offset, scalar_width(storage))?;

    Ok(FieldOutcome::Def(ConstantDef {
        name: name.to_string(),
        page,
        offset,
        kind: ConstantKind::Bits {
            storage,
            bit_lo,
            bit_hi,
            options,
        },
        scale: Number::Lit(1.0),
        translate: Number::Lit(0.0),
        units: String::new(),
        low: Number::Lit(0.0),
        high: Number::Lit(0.0),
        digits: 0,
    }))
}

/// `name = string, ENCODING, offset, LENGTH`
///
/// Field order ported structurally from `adbancroft/TunerStudioIniParser`
/// (LGPLv3) — see module doc comment. Only `ASCII` encoding is recognised;
/// the encoding token itself isn't otherwise interpreted since
/// [`ConstantKind::Text`] only tracks length.
fn parse_string(
    name: &str,
    fields: &[String],
    page: u16,
    running_offset: &mut OffsetCounter,
) -> crate::Result<FieldOutcome> {
    let offset = match fields
        .get(2)
        .and_then(|s| resolve_offset(s, running_offset))
    {
        Some(OffsetResolution::Value(v)) => v,
        Some(OffsetResolution::Poisoned) => return Ok(FieldOutcome::PoisonedOffset),
        None => return Err(invalid(name, "unparseable offset")),
    };
    let len = fields
        .get(3)
        .and_then(|s| s.trim().parse::<usize>().ok())
        .ok_or_else(|| invalid(name, "unparseable string length"))?;

    *running_offset = checked_running_offset(name, offset, len)?;

    Ok(FieldOutcome::Def(ConstantDef {
        name: name.to_string(),
        page,
        offset,
        kind: ConstantKind::Text { len },
        scale: Number::Lit(1.0),
        translate: Number::Lit(0.0),
        units: String::new(),
        low: Number::Lit(0.0),
        high: Number::Lit(0.0),
        digits: 0,
    }))
}
