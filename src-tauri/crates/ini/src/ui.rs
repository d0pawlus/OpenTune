// SPDX-License-Identifier: GPL-3.0-or-later
//! M2 UI-description types — menus, dialogs, tables, curves, and diagnostics.
//!
//! These mirror the `[Menu]`, `[*Dialog]`, `[TableEditor]`, and `[CurveEditor]`
//! sections of a TunerStudio-class INI. `visible`/`enable` expressions are
//! kept as raw strings and evaluated later by the Task 2 `expr` evaluator.

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

/// A 2-D or 3-D table editor definition (e.g. a fuel or ignition map).
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct TableDef {
    /// The table's display/internal name.
    pub name: String,
    /// The name of the constant providing the X-axis bin values.
    pub x_bins: String,
    /// The name of the constant providing the Y-axis bin values.
    pub y_bins: String,
    /// The name of the constant providing the Z (cell) values.
    pub z: String,
}

/// A 2-D curve editor definition (e.g. a warmup enrichment curve).
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct CurveDef {
    /// The curve's display/internal name.
    pub name: String,
    /// The name of the constant providing the X-axis values.
    pub x_bins: String,
    /// The name of the constant providing the Y-axis values.
    pub y_bins: String,
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
