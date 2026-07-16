// SPDX-License-Identifier: GPL-3.0-or-later
//! `[OutputChannels]` section parser — sub-step 2.4.
//!
//! Port source (ADR-0006): [`hyper-tuner/ini`](https://github.com/hyper-tuner/ini)
//! (MIT) — `parseOutputChannels` (`src/ini.ts`, ~lines 235-266) tries the
//! same `parseConstAndVar` shape already ported for `[Constants]` in M2
//! (`constants_fields.rs`) first, reached via the "short" scalar/bits
//! branches (no trailing `min, max, digits`, unlike `[Constants]`); on
//! failure it falls back to a generic `name = value` parse that captures
//! `ochGetCommand`, `ochBlockSize`, and computed `{ expr }` channels
//! (`coolant = { coolantRaw - 40 }`, `throttle = { tps }, "%"`) verbatim.
//!
//! This module mirrors that shape structurally (split scalar/bits/computed
//! by inspecting the RHS rather than a parser-combinator grammar, per this
//! crate's existing `constants_parser.rs` style) rather than porting the
//! parser-combinator machinery line-for-line.
//!
//! Extension (written fresh, not in hyper-tuner): computed channels' `{ expr
//! }` is stored as an opaque string; expression *evaluation* (resolving it
//! against sibling channel values) is deferred to Task 6. Unknown entry
//! kinds degrade gracefully (`Diagnostic` + continue) rather than
//! hyper-tuner's `.tryParse` throw, matching this project's contract.
//!
//! `ochGetCommand` and `ochBlockSize` are header keys, not channel entries —
//! they are skipped here (handled by `parser::parse_comms`) so they don't
//! get misparsed as unknown channel kinds.

use crate::constants_fields::{
    parse_bit_range, parse_scalar_type, scalar_width, split_fields, unquote,
};
use crate::{Diagnostic, IniError, OutputChannelDef};

/// The result of parsing the `[OutputChannels]` section.
pub struct ParsedOutputChannels {
    pub channels: Vec<OutputChannelDef>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Header keys inside `[OutputChannels]` that are comms settings, not
/// channel entries — parsed separately by `parser::parse_comms`.
const HEADER_KEYS: [&str; 2] = ["ochGetCommand", "ochBlockSize"];

/// Parse every `[OutputChannels]` section in the (already-preprocessed) INI
/// text.
pub fn parse_output_channels(
    ini_text: &str,
    och_block_size: u32,
) -> crate::Result<ParsedOutputChannels> {
    let mut channels = Vec::new();
    let mut diagnostics = Vec::new();
    let mut in_section = false;

    for raw_line in ini_text.lines() {
        let line = raw_line.trim();

        if line.is_empty() || line.starts_with(';') {
            continue;
        }

        if let Some(inner) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            in_section = inner.trim() == "OutputChannels";
            continue;
        }

        if !in_section {
            continue;
        }

        let line = strip_inline_comment(line);
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        let name = name.trim();
        let value = strip_inline_comment(value).trim();

        if HEADER_KEYS.contains(&name) {
            continue;
        }

        match parse_channel_line(name, value) {
            Some(def) => {
                validate_channel_bounds(&def, och_block_size)?;
                channels.push(def);
            }
            None => diagnostics.push(unrecognised(name)),
        }
    }

    Ok(ParsedOutputChannels {
        channels,
        diagnostics,
    })
}

/// Parse one `name = ...` line inside `[OutputChannels]`.
///
/// Returns `None` for an unrecognised construct so the caller can degrade
/// gracefully with a `Diagnostic` instead of failing the whole parse.
fn parse_channel_line(name: &str, value: &str) -> Option<OutputChannelDef> {
    if let Some(rest) = value.trim_start().strip_prefix('{') {
        return parse_computed(name, rest);
    }

    let fields = split_fields(value);
    match fields.first().map(String::as_str) {
        Some("scalar") => parse_scalar(name, &fields),
        Some("bits") => parse_bits(name, &fields),
        _ => None,
    }
}

/// `name = { expr }` or `name = { expr }, "units"`.
///
/// Receives the text after the opening `{`: everything up to the first `}`
/// (located with `find('}')`) is the expression, and an optional `, "units"`
/// tail after the closing brace is unquoted into `units`.
fn parse_computed(name: &str, after_brace: &str) -> Option<OutputChannelDef> {
    let closing = after_brace.find('}')?;
    let expr = after_brace[..closing].trim().to_string();
    let tail = after_brace[closing + 1..].trim();
    // tail is either empty, or `, "units"` (possibly with more trailing
    // commas we don't expect but tolerate by taking only the first field).
    let units = tail
        .strip_prefix(',')
        .map(|s| unquote(s.trim()))
        .unwrap_or_default();

    Some(OutputChannelDef::Computed {
        name: name.to_string(),
        expr,
        units,
    })
}

/// `name = scalar, TYPE, offset, "units", scale, translate` (no min/max/digits).
fn parse_scalar(name: &str, fields: &[String]) -> Option<OutputChannelDef> {
    let kind = fields.get(1).and_then(|s| parse_scalar_type(s))?;
    let offset = fields.get(2)?.trim().parse::<usize>().ok()?;
    let units = fields.get(3).map(|s| unquote(s)).unwrap_or_default();
    let scale = fields
        .get(4)
        .and_then(|s| s.trim().parse::<f64>().ok())
        .unwrap_or(1.0);
    let translate = fields
        .get(5)
        .and_then(|s| s.trim().parse::<f64>().ok())
        .unwrap_or(0.0);

    Some(OutputChannelDef::Scalar {
        name: name.to_string(),
        kind,
        offset,
        units,
        scale,
        translate,
    })
}

/// `name = bits, TYPE, offset, [lo:hi]`.
fn parse_bits(name: &str, fields: &[String]) -> Option<OutputChannelDef> {
    let storage = fields.get(1).and_then(|s| parse_scalar_type(s))?;
    let offset = fields.get(2)?.trim().parse::<usize>().ok()?;
    // Output channels have no labels, so a MegaTune `+N` display offset
    // (third tuple element) has nothing to apply to — drop it.
    let (bit_lo, bit_hi, _) = fields.get(3).and_then(|s| parse_bit_range(s))?;

    Some(OutputChannelDef::Bits {
        name: name.to_string(),
        storage,
        offset,
        bit_lo,
        bit_hi,
    })
}

fn validate_channel_bounds(def: &OutputChannelDef, och_block_size: u32) -> crate::Result<()> {
    let (name, offset, width) = match def {
        OutputChannelDef::Scalar {
            name, kind, offset, ..
        } => (name, *offset, scalar_width(*kind)),
        OutputChannelDef::Bits {
            name,
            storage,
            offset,
            ..
        } => (name, *offset, scalar_width(*storage)),
        OutputChannelDef::Computed { .. } => return Ok(()),
    };

    let end = offset
        .checked_add(width)
        .ok_or_else(|| IniError::InvalidValue {
            key: name.clone(),
            detail: "output-channel offset + width overflows platform limits".to_string(),
        })?;

    if och_block_size != 0 {
        let block_size = usize::try_from(och_block_size).map_err(|_| IniError::InvalidValue {
            key: "ochBlockSize".to_string(),
            detail: format!("{och_block_size} does not fit this platform's address space"),
        })?;
        if end > block_size {
            return Err(IniError::InvalidValue {
                key: name.clone(),
                detail: format!(
                    "output-channel offset {offset} + width {width} exceeds ochBlockSize {block_size}"
                ),
            });
        }
    }

    Ok(())
}

fn unrecognised(name: &str) -> Diagnostic {
    Diagnostic {
        section: "OutputChannels".to_string(),
        detail: format!("unrecognised output channel entry for `{name}`"),
    }
}

/// Strip a trailing `; …` inline comment, honoring quoted strings and
/// brace-expressions (mirrors `constants_parser::strip_inline_comment`;
/// `pub(crate)` so `gauges_parser` shares it instead of adding a third copy).
pub(crate) fn strip_inline_comment(s: &str) -> &str {
    let mut in_quote = false;
    let mut brace_depth = 0u32;
    for (i, ch) in s.char_indices() {
        match ch {
            '"' => in_quote = !in_quote,
            '{' if !in_quote => brace_depth += 1,
            '}' if !in_quote => brace_depth = brace_depth.saturating_sub(1),
            ';' if !in_quote && brace_depth == 0 => return &s[..i],
            _ => {}
        }
    }
    s
}
