// SPDX-License-Identifier: GPL-3.0-or-later
//! `[Constants]` / `[PcVariables]` section parser — sub-step 1.4.
//!
//! Port source (ADR-0006): [`hyper-tuner/ini`](https://github.com/hyper-tuner/ini)
//! (MIT) — `parseConstants` establishes the `page = N` running-page-number
//! convention this module walks. Per-line field parsing (the ported
//! `parseConstAndVar` field order, plus the fresh `lastOffset` and string
//! handling) lives in `constants_fields.rs`; see that module's doc comment
//! for the full port/gap breakdown.
//!
//! Gap filled here (written fresh — absent from hyper-tuner): graceful
//! degradation (`Diagnostic` + continue) on an unrecognised constant
//! `class` and on an offset that overflows its page — hyper-tuner's
//! `.tryParse` throws on any unrecognised line and never validates page
//! bounds; this project's contract requires the parse to continue past
//! unknown constructs and to hard-fail only on the specific overflow case
//! (see `IniError`/`Diagnostic` conventions in `parser.rs`).

use crate::constants_fields::{parse_constant_line, ConstantLineResult, OffsetCounter};
use crate::{ConstantDef, ConstantKind, Diagnostic, Endianness, IniError, PageDef};
use std::collections::HashMap;

/// The result of parsing the `[Constants]`/`[PcVariables]` sections.
pub struct ParsedConstants {
    pub pages: Vec<PageDef>,
    pub constants: Vec<ConstantDef>,
    pub pc_variables: Vec<ConstantDef>,
    pub endianness: Option<Endianness>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(PartialEq, Eq)]
enum Section {
    Constants,
    PcVariables,
    Other,
}

/// Parse every `[Constants]` and `[PcVariables]` section in the
/// (already-preprocessed) INI text.
///
/// Unlike [`crate::parse_comms`], this walks the *whole* file rather than
/// stopping at the first match, because `page = N` state and `lastOffset`
/// counters must accumulate across the entire `[Constants]` body.
pub fn parse_constants(ini_text: &str) -> crate::Result<ParsedConstants> {
    let mut pages: Vec<PageDef> = Vec::new();
    let mut constants: Vec<ConstantDef> = Vec::new();
    let mut pc_variables: Vec<ConstantDef> = Vec::new();
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut endianness: Option<Endianness> = None;

    let mut section = Section::Other;
    let mut current_page: u16 = 0;
    let mut last_offset_by_page: HashMap<u16, OffsetCounter> = HashMap::new();

    for raw_line in ini_text.lines() {
        let line = raw_line.trim();

        if line.is_empty() || line.starts_with(';') {
            continue;
        }

        if let Some(inner) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            section = match inner.trim() {
                "Constants" => Section::Constants,
                "PcVariables" => Section::PcVariables,
                _ => Section::Other,
            };
            continue;
        }

        if section == Section::Other {
            continue;
        }

        let line = strip_inline_comment(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = strip_inline_comment(value).trim();

        match key {
            "page" => {
                current_page = value.trim().parse::<u16>().unwrap_or(current_page + 1);
                continue;
            }
            "nPages" => continue, // informational; pages come from pageSize
            "pageSize" => {
                pages = value
                    .split(',')
                    .enumerate()
                    .map(|(i, tok)| PageDef {
                        number: (i + 1) as u16,
                        size: tok.trim().parse::<usize>().unwrap_or(0),
                    })
                    .collect();
                continue;
            }
            "endianness" => {
                endianness = parse_endianness(value);
                continue;
            }
            // TunerStudio metadata / comms keys living in [Constants] (real
            // speeduino.ini l.240-274). Comms keys are consumed by
            // `parse_comms`' scattered scan; the rest are recorded-deferred
            // (m4-decisions) — neither is an unknown *constant*, so no
            // diagnostic and no page-counter poison.
            "pageIdentifier"
            | "pageReadCommand"
            | "pageValueWrite"
            | "burnCommand"
            | "blockingFactor"
            | "blockReadTimeout"
            | "interWriteDelay"
            | "pageActivationDelay"
            | "messageEnvelopeFormat"
            | "crc32CheckCommand"
            | "tableCrcCommand"
            | "pageChunkWrite"
            | "tsWriteBlocks"
            | "delayAfterPortOpen"
            | "readSdCompressed"
            | "restrictSquirtRelationship" => continue,
            _ => {}
        }

        if section == Section::PcVariables {
            // [PcVariables] has no offset field in its grammar, so a fresh
            // Known(0) counter is a throwaway — `lastOffset`/poisoning never
            // apply here (see `parse_scalar_no_offset`).
            match parse_constant_line(key, value, None, &mut OffsetCounter::zero()) {
                Ok(ConstantLineResult::Def(def)) => pc_variables.push(def),
                Ok(ConstantLineResult::UnknownClass) => {
                    diagnostics.push(unrecognised("PcVariables", key));
                }
                Ok(ConstantLineResult::PoisonedOffset) => {
                    diagnostics.push(unrecognised("PcVariables", key));
                }
                Err(e) => return Err(e),
            }
            continue;
        }

        // section == Section::Constants
        let running = last_offset_by_page
            .entry(current_page)
            .or_insert_with(OffsetCounter::zero);
        let result = parse_constant_line(key, value, Some(current_page), running);
        match result {
            Ok(ConstantLineResult::Def(def)) => match validate_offset_within_page(&def, &pages)? {
                OffsetCheck::WithinPage => constants.push(def),
                OffsetCheck::Overflow => {
                    diagnostics.push(overflow_diagnostic("Constants", &def.name));
                }
            },
            Ok(ConstantLineResult::UnknownClass) => {
                diagnostics.push(unrecognised("Constants", key));
                // The unknown constant's size is unknowable, so the running
                // offset can no longer be trusted for this page: poison it
                // rather than silently desyncing every later `lastOffset`
                // constant onto a wrong-but-plausible offset.
                last_offset_by_page.insert(current_page, OffsetCounter::Poisoned);
            }
            Ok(ConstantLineResult::PoisonedOffset) => {
                diagnostics.push(poisoned_offset("Constants", key));
            }
            Err(e) => return Err(e),
        }
    }

    Ok(ParsedConstants {
        pages,
        constants,
        pc_variables,
        endianness,
        diagnostics,
    })
}

fn unrecognised(section: &str, name: &str) -> Diagnostic {
    Diagnostic {
        section: section.to_string(),
        detail: format!("unrecognised constant class for `{name}`"),
    }
}

/// A `lastOffset` constant whose page-running offset was poisoned by an
/// earlier unrecognised constant on the same page (see [`OffsetCounter`]).
/// The constant is skipped — never added with a desynced offset.
fn poisoned_offset(section: &str, name: &str) -> Diagnostic {
    Diagnostic {
        section: section.to_string(),
        detail: format!(
            "`{name}`'s lastOffset is unreliable: an earlier unrecognised constant \
             on this page prevented tracking the running offset"
        ),
    }
}

/// Strip a trailing `; …` inline comment, honoring quoted strings and
/// brace-expressions (a `;` can never legally appear inside either in
/// this grammar, but we still track quote state to avoid truncating a
/// units string like `"m/s; approx"`).
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

fn parse_endianness(value: &str) -> Option<Endianness> {
    match value.trim().to_ascii_lowercase().as_str() {
        "little" => Some(Endianness::Little),
        "big" => Some(Endianness::Big),
        _ => None,
    }
}

/// The outcome of [`validate_offset_within_page`].
enum OffsetCheck {
    /// `offset + size` fits within the declared page (or the page number is
    /// unknown, in which case there is nothing to validate against).
    WithinPage,
    /// `offset + size` (or the size computation itself) overflowed `usize`
    /// arithmetic. `offset`/shape `rows`/`cols` are unbounded `usize`
    /// literals straight from untrusted INI text (see `resolve_offset`/
    /// `parse_shape` in `constants_fields.rs`) — fail OPEN, same spirit as
    /// every other malformed constant line: a `Diagnostic` and skip, never a
    /// panic (debug `overflow-checks`) or a silent wrap (release) that could
    /// let a bogus offset slip past the very check meant to catch it.
    Overflow,
}

/// A constant's `offset + size` must not exceed its page's declared size.
fn validate_offset_within_page(def: &ConstantDef, pages: &[PageDef]) -> crate::Result<OffsetCheck> {
    let Some(page) = pages.iter().find(|p| p.number == def.page) else {
        return Ok(OffsetCheck::WithinPage); // Unknown page number — nothing to validate against.
    };
    let Some(size) = constant_byte_size(def) else {
        return Ok(OffsetCheck::Overflow);
    };
    match def.offset.checked_add(size) {
        Some(end) if end <= page.size => Ok(OffsetCheck::WithinPage),
        Some(_) => Err(IniError::InvalidValue {
            key: def.name.clone(),
            detail: format!(
                "offset {} + size {} exceeds page {} size {}",
                def.offset, size, page.number, page.size
            ),
        }),
        None => Ok(OffsetCheck::Overflow),
    }
}

/// The constant's total byte footprint, or `None` if computing it overflows
/// `usize` (an array whose attacker-controlled `rows * cols` doesn't fit).
fn constant_byte_size(def: &ConstantDef) -> Option<usize> {
    use crate::constants_fields::scalar_width;
    match &def.kind {
        ConstantKind::Scalar(t) => Some(scalar_width(*t)),
        ConstantKind::Array { elem, shape } => scalar_width(*elem)
            .checked_mul(shape.rows)
            .and_then(|rows_size| rows_size.checked_mul(shape.cols)),
        ConstantKind::Bits { storage, .. } => Some(scalar_width(*storage)),
        ConstantKind::Text { len } => Some(*len),
    }
}

/// A constant's declared offset/size overflowed the page-bounds check itself
/// — the size is unknowable, so (unlike a plain `UnknownClass`) there is no
/// well-formed `ConstantDef` to store; skip it with a diagnostic.
fn overflow_diagnostic(section: &str, name: &str) -> Diagnostic {
    Diagnostic {
        section: section.to_string(),
        detail: format!(
            "`{name}`'s declared offset/size overflows the page-bounds check \
             (offset or array shape too large); skipped"
        ),
    }
}
