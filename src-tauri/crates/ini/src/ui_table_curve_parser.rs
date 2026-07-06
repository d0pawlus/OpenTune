// SPDX-License-Identifier: GPL-3.0-or-later
//! `[TableEditor]`/`[CurveEditor]` section parser — split out of
//! `ui_parser.rs` to keep each file focused (see sub-step 3.3; full grammar
//! landed M4 Task 2).
//!
//! Port source (ADR-0006): [`hyper-tuner/ini`](https://github.com/hyper-tuner/ini)
//! (MIT © 2021 Piotr Rogowski) — `parseTables`/`parseCurves` and
//! `types/src/types/config.ts:153-176` establish the full field set ported
//! here:
//! - `Table`: `map, title, page, help?, xBins[], yBins[], zBins[],
//!   xyLabels[], gridHeight, gridOrient[], upDownLabel[]`.
//! - `Curve`: `title, labels[], xAxis[], yAxis[], xBins[], yBins[], size[],
//!   gauge?`.
//!
//! `xBins`/`yBins` carry an optional 2nd token — a "display channel" driving
//! the live cursor in TunerStudio's table view (e.g. `xBins = rpmBins, rpm`).
//! hyper-tuner captures-but-never-uses it (dead code on the TS side); this
//! port promotes it to a real field (`x_channel`/`y_channel` on
//! [`TableDef`], `x_channel` on [`CurveDef`]) as our own recorded extension —
//! the frontend (Task 5/6) needs it to draw the live cursor.
//!
//! Cross-reference checks: a bin or Z/map name that isn't found in either
//! `[Constants]` or `[PcVariables]` records a [`Diagnostic`] and keeps the
//! raw name in the produced `TableDef`/`CurveDef` — it never panics and
//! never fails the whole parse (the M2 contract's graceful-degradation
//! rule). Widened to search both namespaces in M4 Task 2 (originally
//! `[Constants]`-only): the real file's warmup-analyzer curves reference
//! `[PcVariables]`-scoped axes (`wueAFR`, `wueRecommended`) — see
//! `docs/notes/m4-decisions.md`. This applies symmetrically to both tables
//! and curves — a curve's `xBins`/`yBins` names a constant exactly like a
//! table's does.
//!
//! Not representable under either frozen shape (tolerated silently, never
//! diagnosed — matches this module's pre-Task-2 precedent for genuinely
//! unrepresentable per-row metadata): a curve's `topicHelp` (only
//! [`TableDef`] carries `help`) and `lineLabel` (a per-series legend for a
//! curve with more than one `yBins`; real file l.4922-4923's
//! `warmup_analyzer_curve` overlays a second, read-only series this way —
//! the frozen [`CurveDef`] has a single `y_bins` slot). **Corrected in M4
//! Task 6** (sanctioned fold-in; see `docs/notes/m4-decisions.md`'s Task 6
//! section): a repeated `yBins` now keeps the FIRST occurrence rather than
//! the last, so `y_bins` stays bound to the editable `[Constants]` series
//! (`wueRates`) instead of the read-only `[PcVariables]` analyzer output
//! (`wueRecommended`) — the frozen `CurveDef::y_bins` doc contract is "the
//! editable data array". `xBins` and every other single-valued curve/table
//! attribute in this module are unaffected and remain last-wins (they never
//! repeat in the real file); `lineLabel` stays silently dropped until
//! `CurveDef` grows a real multi-series representation.

use crate::constants_fields::parse_number;
use crate::ui::{CurveAxis, CurveDef, Diagnostic, TableDef};
use crate::ui_tokens::{split_tokens, unquote};
use crate::ConstantDef;

pub(crate) fn parse_table_line(
    key: &str,
    value: &str,
    tables: &mut Vec<TableDef>,
    constants: &[ConstantDef],
    pc_variables: &[ConstantDef],
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
                map3d_id: tokens.get(1).cloned().unwrap_or_default(),
                title: tokens.get(2).map(|t| unquote(t)).unwrap_or_default(),
                page: tokens
                    .get(3)
                    .and_then(|t| t.trim().parse::<u32>().ok())
                    .unwrap_or(0),
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
        "xBins" => set_table_bin(
            value,
            tables,
            constants,
            pc_variables,
            diagnostics,
            TableBin::X,
        ),
        "yBins" => set_table_bin(
            value,
            tables,
            constants,
            pc_variables,
            diagnostics,
            TableBin::Y,
        ),
        "zBins" => set_table_bin(
            value,
            tables,
            constants,
            pc_variables,
            diagnostics,
            TableBin::Z,
        ),
        "topicHelp" => set_table_attr(tables, |t| t.help = unquote(value)),
        "xyLabels" => set_table_attr(tables, |t| t.xy_labels = quoted_list(value)),
        "gridHeight" => set_table_attr(tables, |t| {
            t.grid_height = value.trim().parse::<f64>().unwrap_or(0.0);
        }),
        "gridOrient" => set_table_attr(tables, |t| t.grid_orient = float_list(value)),
        "upDownLabel" => set_table_attr(tables, |t| t.up_down_label = quoted_list(value)),
        _ => {} // any other key: no frozen target under TableDef; tolerated silently.
    }
}

#[derive(Clone, Copy)]
enum TableBin {
    X,
    Y,
    Z,
}

/// Apply an attribute to the most recently declared table (attributes always
/// follow their `table =` header). No table yet ⇒ silently ignore (the M2
/// graceful rule; the header itself already diagnosed if malformed).
fn set_table_attr(tables: &mut [TableDef], apply: impl FnOnce(&mut TableDef)) {
    if let Some(t) = tables.last_mut() {
        apply(t);
    }
}

/// `"RPM", "Fuel Load: "` → unquoted strings, order preserved.
fn quoted_list(value: &str) -> Vec<String> {
    split_tokens(value).iter().map(|t| unquote(t)).collect()
}

/// `250, 0, 340` → floats; unparseable tokens are skipped (graceful).
fn float_list(value: &str) -> Vec<f64> {
    split_tokens(value)
        .iter()
        .filter_map(|t| t.trim().parse::<f64>().ok())
        .collect()
}

/// Whether `name` is declared as either a `[Constants]` or `[PcVariables]`
/// entry — table/curve bin names may reference either namespace (M4 Task 2:
/// widened from `[Constants]`-only, see module doc comment).
fn is_known_constant(name: &str, constants: &[ConstantDef], pc_variables: &[ConstantDef]) -> bool {
    constants.iter().any(|c| c.name == name) || pc_variables.iter().any(|c| c.name == name)
}

/// `xBins`/`yBins = binName, displayChannel` (only the first token is the
/// referenced constant name; the 2nd, when present, is the live-cursor
/// display channel — captured into `x_channel`/`y_channel`); `zBins =
/// binName` (single token, no channel). Cross-checks the referenced name
/// against `constants`/`pc_variables` and records a `Diagnostic` — never
/// panics, never fails the parse — when it's missing from both.
fn set_table_bin(
    value: &str,
    tables: &mut [TableDef],
    constants: &[ConstantDef],
    pc_variables: &[ConstantDef],
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
    if !is_known_constant(name, constants, pc_variables) {
        diagnostics.push(Diagnostic {
            section: "TableEditor".to_string(),
            detail: format!(
                "table `{}` references unknown constant `{name}`",
                table.name
            ),
        });
    }
    match which {
        TableBin::X => {
            table.x_bins = name.clone();
            table.x_channel = tokens.get(1).cloned().unwrap_or_default();
        }
        TableBin::Y => {
            table.y_bins = name.clone();
            table.y_channel = tokens.get(1).cloned().unwrap_or_default();
        }
        TableBin::Z => table.z = name.clone(),
    }
}

pub(crate) fn parse_curve_line(
    key: &str,
    value: &str,
    curves: &mut Vec<CurveDef>,
    constants: &[ConstantDef],
    pc_variables: &[ConstantDef],
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
                title: tokens.get(1).map(|t| unquote(t)).unwrap_or_default(),
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
        "xBins" => set_curve_bin(value, curves, constants, pc_variables, diagnostics, true),
        "yBins" => set_curve_bin(value, curves, constants, pc_variables, diagnostics, false),
        "columnLabel" => set_curve_attr(curves, |c| c.column_labels = quoted_list(value)),
        "xAxis" => set_curve_attr(curves, |c| c.x_axis = parse_curve_axis(value)),
        "yAxis" => set_curve_attr(curves, |c| c.y_axis = parse_curve_axis(value)),
        "gauge" => set_curve_attr(curves, |c| {
            c.gauge = split_tokens(value).first().cloned().unwrap_or_default()
        }),
        "size" => set_curve_attr(curves, |c| c.size = float_list(value)),
        // `topicHelp` (CurveDef has no `help` field) and `lineLabel` (a
        // multi-series legend — see module doc comment) have no
        // representable target; tolerated silently (fail-open).
        _ => {}
    }
}

/// Mirrors `set_table_attr` for the current curve.
fn set_curve_attr(curves: &mut [CurveDef], apply: impl FnOnce(&mut CurveDef)) {
    if let Some(c) = curves.last_mut() {
        apply(c);
    }
}

/// `xAxis`/`yAxis = min, max, divisions` — min/max may be `{ expr }` (real
/// file example: `xAxis = 0, { ignLoadMax }, 5`), captured as [`Number`]
/// (see [`crate::constants_fields::parse_number`]); divisions is a plain
/// `u32` grid-division count.
///
/// [`Number`]: crate::Number
fn parse_curve_axis(value: &str) -> Option<CurveAxis> {
    let tokens = split_tokens(value);
    Some(CurveAxis {
        min: parse_number(tokens.first()?),
        max: parse_number(tokens.get(1)?),
        divisions: tokens
            .get(2)
            .and_then(|t| t.trim().parse().ok())
            .unwrap_or(0),
    })
}

/// Mirrors `set_table_bin`'s cross-reference check: a curve's `xBins`/
/// `yBins` names a constant exactly like a table's does, so a missing
/// reference degrades to a `Diagnostic` here too, never a hard error. Only
/// `xBins` carries a live-cursor display channel (2nd token) in the real
/// grammar — `yBins` never does, so `curve.x_channel` is the only channel
/// field on [`CurveDef`].
///
/// `yBins` is the one exception to this module's last-wins default (M4
/// Task 6 fold-in, sanctioned by the controller): a repeated `yBins` keeps
/// the FIRST occurrence, set only when `curve.y_bins` is still empty. The
/// frozen `CurveDef::y_bins` doc contract is "the editable data array", and
/// the real file's only multi-series curve (`warmup_analyzer_curve`,
/// l.4915-4923) declares the editable `[Constants]` series first
/// (`yBins = wueRates`) followed by a read-only `[PcVariables]` analyzer
/// output (`yBins = wueRecommended`) — last-wins would have bound the curve
/// editor to the read-only PC variable. `xBins` (never repeats in the real
/// file) and every diagnostic path below are unaffected — still last-wins /
/// fired on every occurrence, same as before.
fn set_curve_bin(
    value: &str,
    curves: &mut [CurveDef],
    constants: &[ConstantDef],
    pc_variables: &[ConstantDef],
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
    if !is_known_constant(name, constants, pc_variables) {
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
        curve.x_channel = tokens.get(1).cloned().unwrap_or_default();
    } else if curve.y_bins.is_empty() {
        curve.y_bins = name.clone();
    }
}
