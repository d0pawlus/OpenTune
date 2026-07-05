// SPDX-License-Identifier: GPL-3.0-or-later
//! M3 `[GaugeConfigurations]` and `[FrontPage]` types — the default dashboard.
//!
//! `GaugeDef` describes a single gauge's display rules (thresholds, digits,
//! units); `FrontPageDef` lays out the default dashboard's gauge slots and
//! boolean indicators. Real parsing (Task 3) fills these; this module only
//! freezes the shape.

use crate::Number;

/// One `[GaugeConfigurations]` entry. The six numeric bounds may each be a
/// `{ expr }` referencing PcVariables/constants → captured as `Number` (Lit
/// or Expr), reusing the M2 `Number` type. `units` is a text label, not a
/// number — captured as `String`, matching `ConstantDef.units`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct GaugeDef {
    /// gauge id referenced by FrontPage slots
    pub name: String,
    /// the output-channel var it displays (e.g. "rpm")
    pub channel: String,
    pub title: String,
    /// The unit label shown in the UI (e.g. "RPM", "kPa").
    pub units: String,
    pub low: Number,
    pub high: Number,
    pub lo_danger: Number,
    pub lo_warn: Number,
    pub hi_warn: Number,
    pub hi_danger: Number,
    /// vd
    pub value_digits: u8,
    /// ld
    pub label_digits: u8,
    /// gaugeCategory, for grouping menus
    pub category: String,
}

/// `[FrontPage]` — the default dashboard layout.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct FrontPageDef {
    /// gauge1..gauge8 → GaugeConfigurations names, in slot order (2 rows × 4).
    pub gauge_slots: Vec<String>,
    /// `indicator = { expr }, "off", "on", offBg, offFg, onBg, onFg`.
    pub indicators: Vec<IndicatorDef>,
}

/// One `[FrontPage]` `indicator` entry.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct IndicatorDef {
    /// bare bit-channel or comparison; evaluated by realtime
    pub expr: String,
    pub off_label: String,
    pub on_label: String,
    /// named color, verbatim
    pub off_bg: String,
    pub off_fg: String,
    pub on_bg: String,
    pub on_fg: String,
}
