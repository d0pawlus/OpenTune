// SPDX-License-Identifier: GPL-3.0-or-later
//! `[VeAnalyze]` section parser — M4 Task 2.
//!
//! **Written fresh** (ADR-0006 recorded exception): hyper-tuner/ini's
//! section switch ends at `[Datalog]` (`src/ini.ts` ~lines 146-190) — it
//! never parses `[VeAnalyze]`, so there is nothing to port. The grammar
//! truth-source is the real `speeduino.ini` (noisymime/speeduino @
//! 0832dc1d, GPL-3 — consulted as a reference corpus only, no code
//! involved), quoted verbatim on [`crate::ve_analyze::VeAnalyzeDef`]'s doc
//! comment.
//!
//! `[WueAnalyze]` (a distinct section — the warmup-enrichment analog of
//! `[VeAnalyze]`) and the `lambdaTargetTables` key are recorded-deferred
//! (`docs/notes/m4-decisions.md`): silently skipped, not represented under
//! [`VeAnalyzeDef`]. Malformed rows degrade to a [`Diagnostic`], never an
//! error — the M2 graceful-degradation contract.

use crate::constants_fields::{split_fields, unquote};
use crate::output_channels_parser::strip_inline_comment;
use crate::ui::Diagnostic;
use crate::ve_analyze::{AnalyzeFilterDef, FilterOp, VeAnalyzeDef, VeAnalyzeMapDef};

/// The result of parsing the `[VeAnalyze]` section.
pub(crate) struct ParsedVeAnalyze {
    pub def: Option<VeAnalyzeDef>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Parse the `[VeAnalyze]` section from preprocessed INI text (`#else`
/// branches already resolved by [`crate::preprocess`]). `[WueAnalyze]` and
/// `lambdaTargetTables` are recorded-deferred (silently skipped — see module
/// doc comment). Malformed rows degrade to a [`Diagnostic`], never an error.
/// Returns `def: None` when the INI declares no `[VeAnalyze]` content at
/// all (mirrors [`crate::gauges::FrontPageDef`]'s "absent section" contract
/// as closely as an `Option` vs. an always-present-but-empty struct allows).
pub(crate) fn parse_ve_analyze(ini_text: &str) -> ParsedVeAnalyze {
    let mut maps = Vec::new();
    let mut filters = Vec::new();
    let mut diagnostics = Vec::new();
    let mut in_section = false;

    for raw in ini_text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if let Some(inner) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            in_section = inner.trim() == "VeAnalyze";
            continue;
        }
        if !in_section {
            continue;
        }

        let line = strip_inline_comment(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "veAnalyzeMap" => {
                let f = split_fields(value.trim());
                if f.len() >= 4 {
                    maps.push(VeAnalyzeMapDef {
                        table: f[0].clone(),
                        target_table: f[1].clone(),
                        lambda_channel: f[2].clone(),
                        ego_channel: f[3].clone(),
                    });
                } else {
                    diagnostics.push(Diagnostic {
                        section: "VeAnalyze".to_string(),
                        detail: format!("malformed veAnalyzeMap: `{value}`"),
                    });
                }
            }
            "lambdaTargetTables" => {} // recorded-deferred (m4-decisions)
            "filter" => match parse_filter(value.trim()) {
                Some(f) => filters.push(f),
                None => diagnostics.push(Diagnostic {
                    section: "VeAnalyze".to_string(),
                    detail: format!("malformed filter: `{value}`"),
                }),
            },
            other => diagnostics.push(Diagnostic {
                section: "VeAnalyze".to_string(),
                detail: format!("unrecognised key `{other}`"),
            }),
        }
    }

    let def = if maps.is_empty() && filters.is_empty() {
        None
    } else {
        Some(VeAnalyzeDef { maps, filters })
    };
    ParsedVeAnalyze { def, diagnostics }
}

/// `std_*` (referenced by name only, auto-built by TunerStudio) → `Std`;
/// else the full `id, "label", channel, op, value, [reserved], [bool]` custom
/// form.
fn parse_filter(value: &str) -> Option<AnalyzeFilterDef> {
    let f = split_fields(value);
    let first = f.first()?;
    if f.len() == 1 && first.starts_with("std_") {
        return Some(AnalyzeFilterDef::Std(first.clone()));
    }
    if f.len() < 5 {
        return None;
    }
    let op = match f[3].trim() {
        "<" => FilterOp::Lt,
        ">" => FilterOp::Gt,
        "=" => FilterOp::Eq,
        "&" => FilterOp::And,
        _ => return None,
    };
    Some(AnalyzeFilterDef::Custom {
        id: f[0].clone(),
        label: unquote(&f[1]),
        channel: f[2].clone(),
        op,
        value: f[4].trim().parse::<f64>().ok()?,
        default_on: f.get(6).map(|b| b.trim() == "true").unwrap_or(true),
    })
}
