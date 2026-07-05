// SPDX-License-Identifier: GPL-3.0-or-later
//! INI comms-settings parser — M1 slice.
//!
//! Ported structure and tokenisation approach from `hyper-tuner/ini` (MIT,
//! ADR-0006). The real Speeduino `speeduino.ini` (GPL-3) was the authoritative
//! reference for keyword names and values; see `tests/fixtures/speeduino_comms.ini`.
//!
//! # What this does
//!
//! Scans the INI text line-by-line for the first `[MegaTune]` or
//! `[TunerStudio]` section and extracts the comms-related keys. All other
//! sections are skipped gracefully. The parse is intentionally minimal — full
//! `[Constants]`/UI sections are M2 (see ROADMAP §M1). One key is the
//! exception: `ochBlockSize` lives in `[OutputChannels]` (M3, Task 2), so it's
//! read via a separate targeted scan — see [`extract_och_block_size`].

use crate::{CommsSettings, Endianness, EnvelopeFormat, IniError, Result};

/// Parse the comms-settings slice from raw INI text.
///
/// Looks for the first `[MegaTune]` or `[TunerStudio]` section and extracts
/// every comms key defined there. Optional keys that are absent receive sensible
/// defaults (documented per field on [`CommsSettings`]). The exception is
/// `och_block_size`, read separately from `[OutputChannels]`.
pub fn parse_comms(ini_text: &str) -> Result<CommsSettings> {
    let mut kv = extract_comms_section(ini_text);
    kv.extend(extract_scattered_comms(ini_text));

    // Required fields
    let signature = require_string(&kv, "signature")?;
    let query_command = require_string(&kv, "queryCommand")?;
    let version_info = require_string(&kv, "versionInfo")?;
    // `ochGetCommand` is not in `SCATTERED_COMMS_KEYS` (it never lives in
    // `[Constants]` in the real file); when `[MegaTune]`/`[TunerStudio]`
    // lacks it entirely, fall back to the `[OutputChannels]` scanner rather
    // than hard-erroring (`parse_definition`'s own override still applies
    // afterwards and takes precedence when both exist).
    let och_get_command = require_string(&kv, "ochGetCommand")
        .or_else(|e| extract_och_get_command(ini_text).ok_or(e))?;
    let page_read_command = require_string(&kv, "pageReadCommand")?;
    let page_value_write = require_string(&kv, "pageValueWrite")?;
    let burn_command = require_string(&kv, "burnCommand")?;
    let blocking_factor = require_u32(&kv, "blockingFactor")?;
    let block_read_timeout_ms = require_u32(&kv, "blockReadTimeout")?;

    // Optional fields with defaults
    let page_activation_delay_ms = opt_u32(&kv, "pageActivationDelay", 0)?;
    let inter_write_delay_ms = opt_u32(&kv, "interWriteDelay", 0)?;
    let endianness = opt_endianness(&kv)?;
    let envelope = opt_envelope(&kv)?;
    let och_block_size = extract_och_block_size(ini_text);

    Ok(CommsSettings {
        signature,
        query_command,
        version_info,
        och_get_command,
        page_read_command,
        page_value_write,
        burn_command,
        blocking_factor,
        page_activation_delay_ms,
        block_read_timeout_ms,
        inter_write_delay_ms,
        endianness,
        envelope,
        och_block_size,
    })
}

/// Read `ochBlockSize` from `[OutputChannels]` — a separate section from
/// `[MegaTune]`/`[TunerStudio]` (where `ochGetCommand` lives), so this is a
/// targeted scan rather than widening `extract_comms_section`'s section
/// match (which would let `[OutputChannels]`'s own `ochGetCommand` line
/// override the MegaTune one via last-wins). Defaults to `0` when absent,
/// per [`CommsSettings::och_block_size`]'s documented default.
fn extract_och_block_size(ini_text: &str) -> u32 {
    extract_output_channels_value(ini_text, "ochBlockSize")
        .and_then(|v| v.trim().parse::<u32>().ok())
        .unwrap_or(0)
}

/// Read `ochGetCommand` from `[OutputChannels]`, if declared there.
///
/// Real speeduino.ini carries a bare `ochGetCommand = "r"` in `[MegaTune]`
/// and the *windowed* template (`r\$tsCanId\x30%2o%2c`) in
/// `[OutputChannels]` — the windowed one is what TunerStudio actually
/// sends, so [`crate::parse_definition`] lets it override the comms-section
/// value (M3 Task 6 blocker a). `parse_comms` itself is left unchanged: its
/// M1 contract is "the first `[MegaTune]`/`[TunerStudio]` section".
pub(crate) fn extract_och_get_command(ini_text: &str) -> Option<String> {
    extract_output_channels_value(ini_text, "ochGetCommand")
}

/// Targeted scan for one `key = value` under `[OutputChannels]` (first
/// occurrence wins). Returns the unquoted value with inline comments
/// stripped, or `None` when the key is absent.
fn extract_output_channels_value(ini_text: &str, key: &str) -> Option<String> {
    let mut in_output_channels = false;

    for raw_line in ini_text.lines() {
        let line = raw_line.trim();

        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }

        if let Some(inner) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            in_output_channels = inner.trim() == "OutputChannels";
            continue;
        }

        if !in_output_channels {
            continue;
        }

        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == key {
                let value = strip_inline_comment(v.trim()).trim();
                return Some(unquote(value).to_string());
            }
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Scattered comms keys (Wall #1) — real speeduino.ini @ 0832dc1d l.240-274
// ---------------------------------------------------------------------------

/// Comms keys the real speeduino.ini scatters into `[Constants]` (l.240-274 @
/// 0832dc1d) instead of `[MegaTune]`/`[TunerStudio]`. Values there may be
/// per-page comma lists (`"p%2i%2o%2c", "p%2i%2o%2c", ...`) — Speeduino uses
/// identical templates for every page, so the first element is taken and a
/// heterogeneous list is fine to ignore (recorded M4 decision).
const SCATTERED_COMMS_KEYS: &[&str] = &[
    "pageReadCommand",
    "pageValueWrite",
    "burnCommand",
    "blockingFactor",
    "blockReadTimeout",
    "interWriteDelay",
    "pageActivationDelay",
    "messageEnvelopeFormat",
];

/// First element of a possibly comma-separated value, honoring double quotes
/// (a comma inside `"..."` does not split). Returns the element verbatim
/// (quotes intact) so the existing `require_*` unquoting applies unchanged.
fn first_list_element(value: &str) -> &str {
    let mut in_quotes = false;
    for (i, ch) in value.char_indices() {
        match ch {
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => return value[..i].trim(),
            _ => {}
        }
    }
    value.trim()
}

/// Collect allowlisted comms keys from `[Constants]` + `[OutputChannels]`,
/// appended AFTER the primary-section pairs so first-wins keeps the
/// `[MegaTune]`/`[TunerStudio]` value when both declare a key.
///
/// The real file also carries trailing `; comment` text on some of these
/// lines (e.g. `blockingFactor = 251 ; Serial buffer is 257 bytes...`,
/// `interWriteDelay = 10 ;Ignored when tsWriteBlocks is on`) — inline
/// comments are stripped the same way `extract_comms_section` strips them
/// (quote-aware, so a comma inside a quoted template survives) *before*
/// splitting on the first top-level comma, or a single-value field like
/// `blockingFactor` would carry its comment straight into `require_u32` and
/// fail to parse as a number.
fn extract_scattered_comms(ini_text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut in_section = false;
    for raw in ini_text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if let Some(inner) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            in_section = matches!(inner.trim(), "Constants" | "OutputChannels");
            continue;
        }
        if !in_section {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if SCATTERED_COMMS_KEYS.contains(&key) {
            let value = strip_inline_comment(value).trim();
            out.push((key.to_string(), first_list_element(value).to_string()));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Section extraction
// ---------------------------------------------------------------------------

/// Returns a vec of `(key, raw_value)` pairs from the first comms section.
///
/// "Raw value" still has surrounding quotes; stripping happens in the accessor
/// helpers. This matches the hyper-tuner/ini approach of deferring
/// interpretation to typed helpers.
fn extract_comms_section(ini_text: &str) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = Vec::new();
    let mut in_comms = false;

    for raw_line in ini_text.lines() {
        let line = raw_line.trim();

        // Blank lines and full-line comments.
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }

        // Section header — `[SectionName]`.
        if line.starts_with('[') {
            if let Some(inner) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                let section = inner.trim();
                in_comms = matches!(section, "MegaTune" | "TunerStudio");
            } else {
                in_comms = false;
            }
            continue;
        }

        if !in_comms {
            continue;
        }

        // Key = value line.
        if let Some((k, v)) = line.split_once('=') {
            let key = k.trim().to_string();
            let value = strip_inline_comment(v.trim()).trim().to_string();
            pairs.push((key, value));
        }
    }

    pairs
}

/// Strip a trailing `; …` inline comment, honoring quoted strings.
fn strip_inline_comment(s: &str) -> &str {
    let mut in_quote = false;
    for (i, ch) in s.char_indices() {
        match ch {
            '"' => in_quote = !in_quote,
            ';' if !in_quote => return &s[..i],
            _ => {}
        }
    }
    s
}

// ---------------------------------------------------------------------------
// Accessor helpers
// ---------------------------------------------------------------------------

fn find_raw<'a>(kv: &'a [(String, String)], key: &str) -> Option<&'a str> {
    // Last definition wins (mirrors TunerStudio behaviour).
    kv.iter()
        .rev()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

/// Strip optional surrounding double-quotes from a raw INI value.
fn unquote(s: &str) -> &str {
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

fn require_string(kv: &[(String, String)], key: &str) -> Result<String> {
    find_raw(kv, key)
        .map(|v| unquote(v).to_string())
        .ok_or_else(|| IniError::MissingKey(key.to_string()))
}

fn require_u32(kv: &[(String, String)], key: &str) -> Result<u32> {
    let raw = find_raw(kv, key).ok_or_else(|| IniError::MissingKey(key.to_string()))?;
    unquote(raw)
        .trim()
        .parse::<u32>()
        .map_err(|e| IniError::InvalidValue {
            key: key.to_string(),
            detail: e.to_string(),
        })
}

fn opt_u32(kv: &[(String, String)], key: &str, default: u32) -> Result<u32> {
    match find_raw(kv, key) {
        None => Ok(default),
        Some(raw) => unquote(raw)
            .trim()
            .parse::<u32>()
            .map_err(|e| IniError::InvalidValue {
                key: key.to_string(),
                detail: e.to_string(),
            }),
    }
}

fn opt_endianness(kv: &[(String, String)]) -> Result<Endianness> {
    match find_raw(kv, "endianness") {
        None => Ok(Endianness::default()),
        Some(raw) => match unquote(raw).to_ascii_lowercase().trim() {
            "little" => Ok(Endianness::Little),
            "big" => Ok(Endianness::Big),
            other => Err(IniError::InvalidValue {
                key: "endianness".to_string(),
                detail: format!("unknown value `{other}`; expected `little` or `big`"),
            }),
        },
    }
}

fn opt_envelope(kv: &[(String, String)]) -> Result<EnvelopeFormat> {
    match find_raw(kv, "messageEnvelopeFormat") {
        None => Ok(EnvelopeFormat::Plain),
        Some(raw) => match unquote(raw).trim() {
            "msEnvelope_1.0" => Ok(EnvelopeFormat::MsEnvelope10),
            "" | "none" => Ok(EnvelopeFormat::Plain),
            other => Err(IniError::InvalidValue {
                key: "messageEnvelopeFormat".to_string(),
                detail: format!("unknown value `{other}`"),
            }),
        },
    }
}
