// SPDX-License-Identifier: GPL-3.0-or-later
//! `[TableEditor]`/`[CurveEditor]` section parser — split out of
//! `ui_parser.rs` to keep each file focused (see sub-step 3.3).
//!
//! Port source (ADR-0006): [`hyper-tuner/ini`](https://github.com/hyper-tuner/ini)
//! (MIT) — `parseTables`/`parseCurves` establish the `table`/`curve` header
//! field order and the `xBins`/`yBins`/`zBins` bin-list convention (only the
//! first token in an `xBins`/`yBins` value is the referenced constant name;
//! a second "display channel" token, e.g. `xBins = rpmBins, rpm`, is
//! informational only and not represented under the frozen [`TableDef`]/
//! [`CurveDef`] shapes).
//!
//! Cross-reference checks: a bin or Z/map name that isn't found in
//! `constants` records a [`Diagnostic`] and keeps the raw name in the
//! produced `TableDef`/`CurveDef` — it never panics and never fails the
//! whole parse (the M2 contract's graceful-degradation rule). This applies
//! symmetrically to both tables and curves — a curve's `xBins`/`yBins`
//! names a constant exactly like a table's does.

use crate::ui::{CurveDef, Diagnostic, TableDef};
use crate::ui_tokens::split_tokens;
use crate::ConstantDef;

pub(crate) fn parse_table_line(
    key: &str,
    value: &str,
    tables: &mut Vec<TableDef>,
    constants: &[ConstantDef],
    diagnostics: &mut Vec<Diagnostic>,
) {
    match key {
        "table" => {
            // table = table_id, map3d_id, "title", page
            let tokens = split_tokens(value);
            let Some(name) = tokens.first() else {
                return;
            };
            tables.push(TableDef {
                name: name.clone(),
                map3d_id: String::new(),
                title: String::new(),
                page: 0,
                x_bins: String::new(),
                x_channel: String::new(),
                y_bins: String::new(),
                y_channel: String::new(),
                z: String::new(),
                xy_labels: Vec::new(),
                grid_height: 0.0,
                grid_orient: Vec::new(),
                up_down_label: Vec::new(),
                help: String::new(),
            });
        }
        "xBins" => set_table_bin(value, tables, constants, diagnostics, TableBin::X),
        "yBins" => set_table_bin(value, tables, constants, diagnostics, TableBin::Y),
        "zBins" => set_table_bin(value, tables, constants, diagnostics, TableBin::Z),
        _ => {} // topicHelp, xyLabels, gridHeight, gridOrient, upDownLabel: no representable target.
    }
}

#[derive(Clone, Copy)]
enum TableBin {
    X,
    Y,
    Z,
}

/// `xBins`/`yBins = binName, displayChannel` (only the first token is the
/// referenced constant name); `zBins = binName` (single token). Cross-
/// checks the referenced name against `constants` and records a
/// `Diagnostic` — never panics, never fails the parse — when it's missing.
fn set_table_bin(
    value: &str,
    tables: &mut [TableDef],
    constants: &[ConstantDef],
    diagnostics: &mut Vec<Diagnostic>,
    which: TableBin,
) {
    let Some(table) = tables.last_mut() else {
        return;
    };
    let tokens = split_tokens(value);
    let Some(name) = tokens.first() else {
        return;
    };
    if !constants.iter().any(|c| &c.name == name) {
        diagnostics.push(Diagnostic {
            section: "TableEditor".to_string(),
            detail: format!(
                "table `{}` references unknown constant `{name}`",
                table.name
            ),
        });
    }
    match which {
        TableBin::X => table.x_bins = name.clone(),
        TableBin::Y => table.y_bins = name.clone(),
        TableBin::Z => table.z = name.clone(),
    }
}

pub(crate) fn parse_curve_line(
    key: &str,
    value: &str,
    curves: &mut Vec<CurveDef>,
    constants: &[ConstantDef],
    diagnostics: &mut Vec<Diagnostic>,
) {
    match key {
        "curve" => {
            // curve = name, "title"
            let tokens = split_tokens(value);
            let Some(name) = tokens.first() else {
                return;
            };
            curves.push(CurveDef {
                name: name.clone(),
                title: String::new(),
                column_labels: Vec::new(),
                x_axis: None,
                y_axis: None,
                x_bins: String::new(),
                x_channel: String::new(),
                y_bins: String::new(),
                gauge: String::new(),
                size: Vec::new(),
            });
        }
        "xBins" => set_curve_bin(value, curves, constants, diagnostics, true),
        "yBins" => set_curve_bin(value, curves, constants, diagnostics, false),
        _ => {} // columnLabel, xAxis, yAxis, size: no representable target.
    }
}

/// Mirrors `set_table_bin`'s cross-reference check: a curve's `xBins`/
/// `yBins` names a constant exactly like a table's does, so a missing
/// reference degrades to a `Diagnostic` here too, never a hard error.
fn set_curve_bin(
    value: &str,
    curves: &mut [CurveDef],
    constants: &[ConstantDef],
    diagnostics: &mut Vec<Diagnostic>,
    is_x: bool,
) {
    let Some(curve) = curves.last_mut() else {
        return;
    };
    let tokens = split_tokens(value);
    let Some(name) = tokens.first() else {
        return;
    };
    if !constants.iter().any(|c| &c.name == name) {
        diagnostics.push(Diagnostic {
            section: "CurveEditor".to_string(),
            detail: format!(
                "curve `{}` references unknown constant `{name}`",
                curve.name
            ),
        });
    }
    if is_x {
        curve.x_bins = name.clone();
    } else {
        curve.y_bins = name.clone();
    }
}
