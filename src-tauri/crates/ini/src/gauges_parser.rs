// SPDX-License-Identifier: GPL-3.0-or-later
//! `[GaugeConfigurations]` + `[FrontPage]` section parser — Task 3 (M3).
//!
//! **Written fresh** (ADR-0006 recorded exception): hyper-tuner/ini's
//! section switch ends at `[Datalog]` (`src/ini.ts` ~lines 146-190) — it
//! parses neither `[GaugeConfigurations]` nor `[FrontPage]`, so there is
//! nothing to port. The grammar truth-source is the real `speeduino.ini`
//! (noisymime/speeduino @ 63fd68e9, GPL-3 — consulted as a reference corpus
//! only, no code involved; the trimmed test fixture records the exact
//! lines exercised):
//!
//! ```text
//! [GaugeConfigurations]
//! gaugeCategory = "Main"                     ; carried down entries
//! name = channel, "title", "units", lo, hi, loD, loW, hiW, hiD, vd, ld
//!
//! [FrontPage]
//! gaugeN    = gaugeName                      ; N gives the slot order
//! indicator = { expr }, "off label", "on label", offBg, offFg, onBg, onFg
//! ```
//!
//! Any of the six numeric bounds may be a `{ expr }` referencing
//! PcVariables/constants → captured as [`crate::Number::Expr`] (reusing the
//! M2 number-or-expression helper); `units` is a verbatim text label.
//! Degradation contract (mirrors M2's table/curve cross-ref checks):
//! malformed rows and unknown constructs record a [`Diagnostic`] and are
//! skipped; a gauge referencing an unknown output channel — or a
//! front-page slot referencing an unknown gauge — records a [`Diagnostic`]
//! but is kept.

use crate::constants_fields::{parse_number, split_fields, unquote};
use crate::output_channels_parser::strip_inline_comment;
use crate::{Diagnostic, FrontPageDef, GaugeDef, IndicatorDef, OutputChannelDef};

/// The result of parsing `[GaugeConfigurations]` + `[FrontPage]`.
pub(crate) struct ParsedGauges {
    pub gauges: Vec<GaugeDef>,
    pub frontpage: FrontPageDef,
    pub diagnostics: Vec<Diagnostic>,
}

/// A gauge row's positional field count up to and including `hiD` —
/// `channel, title, units` plus the six numeric bounds. Rows shorter than
/// this cannot fill [`GaugeDef`]'s bounds and are skipped with a
/// [`Diagnostic`]; the trailing `vd`/`ld` digits default to `0` when
/// absent, matching the `[Constants]` parser's treatment of `digits`.
const GAUGE_REQUIRED_FIELDS: usize = 9;

/// Which section the line walker is currently inside.
enum Section {
    Other,
    Gauges,
    FrontPage,
}

/// Parse every `[GaugeConfigurations]` and `[FrontPage]` section in the
/// (already-preprocessed) INI text. `channels` backs the gauge → output
/// channel cross-reference check.
pub(crate) fn parse_gauges(ini_text: &str, channels: &[OutputChannelDef]) -> ParsedGauges {
    let mut gauges: Vec<GaugeDef> = Vec::new();
    let mut slots: Vec<(usize, String)> = Vec::new();
    let mut indicators: Vec<IndicatorDef> = Vec::new();
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut section = Section::Other;
    let mut category = String::new();

    for raw_line in ini_text.lines() {
        let line = raw_line.trim();

        if line.is_empty() || line.starts_with(';') {
            continue;
        }

        if let Some(inner) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            section = match inner.trim() {
                "GaugeConfigurations" => {
                    category = String::new();
                    Section::Gauges
                }
                "FrontPage" => Section::FrontPage,
                _ => Section::Other,
            };
            continue;
        }

        let line = strip_inline_comment(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = strip_inline_comment(value).trim();

        match section {
            Section::Other => {}
            Section::Gauges => {
                if key == "gaugeCategory" {
                    category = unquote(value);
                } else {
                    match parse_gauge_row(key, value, &category, channels, &mut diagnostics) {
                        Some(def) => gauges.push(def),
                        None => diagnostics.push(malformed_gauge(key)),
                    }
                }
            }
            Section::FrontPage => {
                parse_frontpage_line(key, value, &mut slots, &mut indicators, &mut diagnostics);
            }
        }
    }

    let gauge_slots = ordered_slots(slots, &gauges, &mut diagnostics);

    ParsedGauges {
        gauges,
        frontpage: FrontPageDef {
            gauge_slots,
            indicators,
        },
        diagnostics,
    }
}

/// Parse one `name = channel, "title", "units", lo, hi, loD, loW, hiW, hiD,
/// vd, ld` row. Extra trailing tokens are tolerated (M2 convention).
///
/// Returns `None` for a row too short to fill the six bounds, so the
/// caller degrades with a [`Diagnostic`] instead of failing the parse. A
/// `channel` that is not a known output channel records a cross-reference
/// [`Diagnostic`] but keeps the gauge (mirrors M2's table bin check).
fn parse_gauge_row(
    name: &str,
    value: &str,
    category: &str,
    channels: &[OutputChannelDef],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<GaugeDef> {
    let fields = split_fields(value);
    if fields.len() < GAUGE_REQUIRED_FIELDS {
        return None;
    }

    let channel = fields[0].clone();
    if !channels.iter().any(|c| c.name() == channel) {
        diagnostics.push(Diagnostic {
            section: "GaugeConfigurations".to_string(),
            detail: format!("gauge `{name}` references unknown output channel `{channel}`"),
        });
    }

    let digits = |i: usize| {
        fields
            .get(i)
            .and_then(|s| s.trim().parse::<u8>().ok())
            .unwrap_or(0)
    };

    Some(GaugeDef {
        name: name.to_string(),
        channel,
        title: unquote(&fields[1]),
        units: unquote(&fields[2]),
        low: parse_number(&fields[3]),
        high: parse_number(&fields[4]),
        lo_danger: parse_number(&fields[5]),
        lo_warn: parse_number(&fields[6]),
        hi_warn: parse_number(&fields[7]),
        hi_danger: parse_number(&fields[8]),
        value_digits: digits(9),
        label_digits: digits(10),
        category: category.to_string(),
    })
}

/// Parse one `[FrontPage]` line: `gaugeN = name` collects a numbered slot;
/// `indicator = { expr }, ...` collects an indicator; anything else records
/// a [`Diagnostic`] and is skipped.
fn parse_frontpage_line(
    key: &str,
    value: &str,
    slots: &mut Vec<(usize, String)>,
    indicators: &mut Vec<IndicatorDef>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Some(n) = key
        .strip_prefix("gauge")
        .and_then(|n| n.parse::<usize>().ok())
    {
        slots.push((n, value.to_string()));
        return;
    }

    if key == "indicator" {
        match parse_indicator(value) {
            Some(ind) => indicators.push(ind),
            None => diagnostics.push(Diagnostic {
                section: "FrontPage".to_string(),
                detail: format!("malformed indicator entry `indicator = {value}`"),
            }),
        }
        return;
    }

    diagnostics.push(Diagnostic {
        section: "FrontPage".to_string(),
        detail: format!("unrecognised front-page entry `{key}`"),
    });
}

/// `indicator = { expr }, "off label", "on label", offBg, offFg, onBg, onFg`.
///
/// The `{ expr }` head is mandatory; missing trailing labels/colors degrade
/// to empty strings (real INIs occasionally omit the color tail).
fn parse_indicator(value: &str) -> Option<IndicatorDef> {
    let after_brace = value.trim_start().strip_prefix('{')?;
    let closing = after_brace.find('}')?;
    let expr = after_brace[..closing].trim().to_string();
    let tail = after_brace[closing + 1..].trim();
    let fields = split_fields(tail.strip_prefix(',').unwrap_or(tail));
    let field = |i: usize| fields.get(i).map(|s| unquote(s)).unwrap_or_default();

    Some(IndicatorDef {
        expr,
        off_label: field(0),
        on_label: field(1),
        off_bg: field(2),
        off_fg: field(3),
        on_bg: field(4),
        on_fg: field(5),
    })
}

/// Order collected `gaugeN` slots by their numeric `N` (declaration order
/// is irrelevant; `gauge10` sorts after `gauge9`, unlike a lexicographic
/// sort) and cross-check each referenced gauge name, recording a
/// [`Diagnostic`] for unknown ones while keeping the slot (degrade, don't
/// drop — mirrors the gauge → channel check above).
fn ordered_slots(
    mut slots: Vec<(usize, String)>,
    gauges: &[GaugeDef],
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<String> {
    slots.sort_by_key(|&(n, _)| n);
    for (n, name) in &slots {
        if !gauges.iter().any(|g| &g.name == name) {
            diagnostics.push(Diagnostic {
                section: "FrontPage".to_string(),
                detail: format!("slot `gauge{n}` references unknown gauge `{name}`"),
            });
        }
    }
    slots.into_iter().map(|(_, name)| name).collect()
}

fn malformed_gauge(name: &str) -> Diagnostic {
    Diagnostic {
        section: "GaugeConfigurations".to_string(),
        detail: format!(
            "malformed gauge row for `{name}` (expected at least {GAUGE_REQUIRED_FIELDS} fields)"
        ),
    }
}
