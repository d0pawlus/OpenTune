// SPDX-License-Identifier: GPL-3.0-or-later
//! M2 UI-description types — menus, dialogs, tables, curves, and diagnostics.
//!
//! These mirror the `[Menu]`, `[*Dialog]`, `[TableEditor]`, and `[CurveEditor]`
//! sections of a TunerStudio-class INI. `visible`/`enable` expressions are
//! kept as raw strings and evaluated later by the Task 2 `expr` evaluator.

use crate::Number;

/// A top-level menu, containing an ordered list of items.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct MenuDef {
    /// The menu's display label.
    pub label: String,
    /// The items shown under this menu, in display order.
    pub items: Vec<MenuItem>,
}

/// A single menu entry that opens a named dialog.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct MenuItem {
    /// The item's display label.
    pub label: String,
    /// The name of the [`DialogDef`] this item opens.
    pub dialog: String,
}

/// A dialog: a named window containing a list of fields.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct DialogDef {
    /// The dialog's internal name, referenced by [`MenuItem::dialog`].
    pub name: String,
    /// The dialog's display title.
    pub title: String,
    /// The fields shown in this dialog, in display order.
    pub fields: Vec<DialogField>,
}

/// A single field within a [`DialogDef`].
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct DialogField {
    /// What kind of field this is (bound constant, nested panel, label, or
    /// spacer).
    pub kind: FieldKind,
    /// Raw expression string controlling visibility, evaluated by the Task 2
    /// `expr` evaluator. `None` means always visible.
    pub visible: Option<String>,
    /// Raw expression string controlling whether the field is enabled,
    /// evaluated by the Task 2 `expr` evaluator. `None` means always enabled.
    pub enable: Option<String>,
}

/// The kind of a [`DialogField`].
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub enum FieldKind {
    /// A field bound to a named constant (see [`ConstantDef`](crate::ConstantDef)).
    Constant(String),
    /// A nested panel referencing another dialog by name.
    Panel(String),
    /// A static text label.
    Label(String),
    /// A layout spacer with no bound value.
    Gap,
}

/// A 2-D/3-D table editor definition. Port shape: hyper-tuner/types
/// `Table` (config.ts:153-166, MIT © Piotr Rogowski). Bins stay lazy string
/// names resolved against `[Constants]` by the consumer.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct TableDef {
    /// editor id, e.g. "veTable1Tbl"
    pub name: String,
    /// 3-D map id, e.g. "veTable1Map" ("" when absent)
    pub map3d_id: String,
    /// display title ("" when absent)
    pub title: String,
    /// page number from the table header (0 when absent)
    pub page: u32,
    /// X-axis bin constant name
    pub x_bins: String,
    /// live-cursor output channel (2nd xBins token; "" when absent)
    pub x_channel: String,
    /// Y-axis bin constant name
    pub y_bins: String,
    /// live-cursor output channel (2nd yBins token; "" when absent)
    pub y_channel: String,
    /// cell (Z) array constant name
    pub z: String,
    /// `xyLabels = "RPM", "Fuel Load: "` (empty when absent)
    pub xy_labels: Vec<String>,
    /// `gridHeight` (0.0 when absent)
    pub grid_height: f64,
    /// `gridOrient = 250, 0, 340` (empty when absent)
    pub grid_orient: Vec<f64>,
    /// `upDownLabel = "(RICHER)", "(LEANER)"`
    pub up_down_label: Vec<String>,
    /// `topicHelp` URL ("" when absent)
    pub help: String,
}

/// One `xAxis`/`yAxis` curve attribute: `min, max, gridDivisions`. Min/max may
/// be `{ expr }` in real INIs (e.g. under `#if LAMBDA`) → captured as `Number`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct CurveAxis {
    pub min: Number,
    pub max: Number,
    pub divisions: u32,
}

/// A 2-D curve editor definition (e.g. a warmup enrichment curve).
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct CurveDef {
    /// The curve's display/internal name.
    pub name: String,
    /// `curve = name, "title"` ("" when absent)
    pub title: String,
    /// `columnLabel = "Temp", "Duty %"`
    pub column_labels: Vec<String>,
    pub x_axis: Option<CurveAxis>,
    pub y_axis: Option<CurveAxis>,
    /// The name of the constant providing the X-axis values.
    pub x_bins: String,
    /// live-cursor channel (2nd xBins token; "")
    pub x_channel: String,
    /// The name of the constant providing the Y-axis values (the editable
    /// data array).
    pub y_bins: String,
    /// referenced gauge name ("" when absent)
    pub gauge: String,
    /// `size = 400, 400` (empty when absent)
    pub size: Vec<f64>,
}

/// A note about part of the INI that was skipped or could not be fully
/// parsed, surfaced to the user instead of failing the whole parse.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct Diagnostic {
    /// The INI section the diagnostic relates to (e.g. `"TableEditor"`).
    pub section: String,
    /// A human-readable description of what was skipped or degraded.
    pub detail: String,
}
