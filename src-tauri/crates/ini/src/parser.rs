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
//! `[Constants]`/UI sections are M2 (see ROADMAP §M1).

use crate::{CommsSettings, Endianness, EnvelopeFormat, IniError, Result};

/// Parse the comms-settings slice from raw INI text.
///
/// Looks for the first `[MegaTune]` or `[TunerStudio]` section and extracts
/// every comms key defined there. Optional keys that are absent receive sensible
/// defaults (documented per field on [`CommsSettings`]).
pub fn parse_comms(ini_text: &str) -> Result<CommsSettings> {
    let kv = extract_comms_section(ini_text);

    // Required fields
    let signature = require_string(&kv, "signature")?;
    let query_command = require_string(&kv, "queryCommand")?;
    let version_info = require_string(&kv, "versionInfo")?;
    let och_get_command = require_string(&kv, "ochGetCommand")?;
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
    })
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
