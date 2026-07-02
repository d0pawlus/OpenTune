// SPDX-License-Identifier: GPL-3.0-or-later
//! IPC data-transfer view of a [`Definition`] for the data-driven UI.
//!
//! The frontend renders menus, dialogs, and fields — it does **not** need byte
//! offsets, page numbers, or scale/translate factors (those are backend
//! encoding concerns). This DTO ships only what the UI reads, and does so with
//! JavaScript-safe numeric types: the pinned `specta-typescript` forbids
//! exporting `usize`/`u64`-style integers (precision loss), which the raw
//! `Definition` uses for offsets and shapes. Narrowing to `u32`/`f64` here is
//! both the correct API boundary and what keeps the generated bindings valid.

use opentune_ini::{
    ConstantDef, ConstantKind, Definition, DialogDef, FieldKind, MenuDef, Number, TableDef,
};
use opentune_model::{CellDiff, FieldDiff, Value};

/// The UI-facing projection of a [`Definition`].
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct DefinitionDto {
    /// The firmware signature, for display.
    pub signature: String,
    /// Top-level menus.
    pub menus: Vec<MenuDto>,
    /// Dialogs referenced by menu items and panels.
    pub dialogs: Vec<DialogDto>,
    /// Constants referenced by dialog fields, indexed by name on the frontend.
    pub constants: Vec<ConstantDto>,
    /// Table editors (rendered as a minimal grid in M2; full editor is M4).
    pub tables: Vec<TableDto>,
}

/// A top-level menu.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct MenuDto {
    pub label: String,
    pub items: Vec<MenuItemDto>,
}

/// A menu entry that opens a dialog by name.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct MenuItemDto {
    pub label: String,
    pub dialog: String,
}

/// A dialog and its fields.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct DialogDto {
    pub name: String,
    pub title: String,
    pub fields: Vec<FieldDto>,
}

/// A single field with its raw visibility/enable expressions (evaluated by the
/// backend `eval_conditions` command — one source of truth, no TS port).
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct FieldDto {
    pub kind: FieldKindDto,
    pub visible: Option<String>,
    pub enable: Option<String>,
}

/// The kind of a field (mirrors `FieldKind`, externally tagged).
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub enum FieldKindDto {
    /// Bound to a named constant.
    Constant(String),
    /// A nested panel referencing another dialog by name.
    Panel(String),
    /// A static label.
    Label(String),
    /// A layout spacer.
    Gap,
}

/// A constant's UI metadata (no byte layout).
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct ConstantDto {
    pub name: String,
    pub units: String,
    /// Decimal digits to display for scalar values.
    pub digits: u32,
    /// Lower bound, when a literal (an expression bound resolves to `None` — the
    /// backend still range-checks on write).
    pub low: Option<f64>,
    /// Upper bound, when a literal.
    pub high: Option<f64>,
    pub kind: ConstantKindDto,
}

/// A constant's interpretation kind, trimmed for the UI.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub enum ConstantKindDto {
    /// A single editable scalar.
    Scalar,
    /// An enum-like selector with named options.
    Bits { options: Vec<String> },
    /// A fixed-shape array/table.
    Array { rows: u32, cols: u32 },
    /// A fixed-length text field.
    Text,
}

/// A table editor definition (bin/cell constant references).
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct TableDto {
    pub name: String,
    pub x_bins: String,
    pub y_bins: String,
    pub z: String,
}

// ── Conversions ──────────────────────────────────────────────────────────────

impl From<&Definition> for DefinitionDto {
    fn from(def: &Definition) -> Self {
        Self {
            signature: def.comms.signature.clone(),
            menus: def.menus.iter().map(MenuDto::from).collect(),
            dialogs: def.dialogs.iter().map(DialogDto::from).collect(),
            constants: def.constants.iter().map(ConstantDto::from).collect(),
            tables: def.tables.iter().map(TableDto::from).collect(),
        }
    }
}

impl From<&MenuDef> for MenuDto {
    fn from(m: &MenuDef) -> Self {
        Self {
            label: m.label.clone(),
            items: m
                .items
                .iter()
                .map(|i| MenuItemDto {
                    label: i.label.clone(),
                    dialog: i.dialog.clone(),
                })
                .collect(),
        }
    }
}

impl From<&DialogDef> for DialogDto {
    fn from(d: &DialogDef) -> Self {
        Self {
            name: d.name.clone(),
            title: d.title.clone(),
            fields: d
                .fields
                .iter()
                .map(|f| FieldDto {
                    kind: FieldKindDto::from(&f.kind),
                    visible: f.visible.clone(),
                    enable: f.enable.clone(),
                })
                .collect(),
        }
    }
}

impl From<&FieldKind> for FieldKindDto {
    fn from(k: &FieldKind) -> Self {
        match k {
            FieldKind::Constant(n) => Self::Constant(n.clone()),
            FieldKind::Panel(n) => Self::Panel(n.clone()),
            FieldKind::Label(s) => Self::Label(s.clone()),
            FieldKind::Gap => Self::Gap,
        }
    }
}

impl From<&ConstantDef> for ConstantDto {
    fn from(c: &ConstantDef) -> Self {
        Self {
            name: c.name.clone(),
            units: c.units.clone(),
            digits: u32::from(c.digits),
            low: lit(&c.low),
            high: lit(&c.high),
            kind: ConstantKindDto::from(&c.kind),
        }
    }
}

impl From<&ConstantKind> for ConstantKindDto {
    fn from(k: &ConstantKind) -> Self {
        match k {
            ConstantKind::Scalar(_) => Self::Scalar,
            ConstantKind::Bits { options, .. } => Self::Bits {
                options: options.clone(),
            },
            ConstantKind::Array { shape, .. } => Self::Array {
                rows: shape.rows as u32,
                cols: shape.cols as u32,
            },
            ConstantKind::Text { .. } => Self::Text,
        }
    }
}

impl From<&TableDef> for TableDto {
    fn from(t: &TableDef) -> Self {
        Self {
            name: t.name.clone(),
            x_bins: t.x_bins.clone(),
            y_bins: t.y_bins.clone(),
            z: t.z.clone(),
        }
    }
}

/// A literal bound resolves to `Some`; an expression bound to `None`.
fn lit(n: &Number) -> Option<f64> {
    match n {
        Number::Lit(v) => Some(*v),
        Number::Expr(_) => None,
    }
}

// ── Task 8: tune diff / merge ───────────────────────────────────────────────

/// IPC projection of [`opentune_model::FieldDiff`] — identical shape, but
/// (transitively, via [`CellDiffDto`]) narrows the `usize` cell index to
/// `u32`, since the pinned `specta-typescript` forbids exporting `usize`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct FieldDiffDto {
    pub name: String,
    pub a: Value,
    pub b: Value,
    pub cells: Vec<CellDiffDto>,
}

/// IPC projection of [`opentune_model::CellDiff`].
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct CellDiffDto {
    pub index: u32,
    pub a: f64,
    pub b: f64,
}

impl From<FieldDiff> for FieldDiffDto {
    fn from(d: FieldDiff) -> Self {
        Self {
            name: d.name,
            a: d.a,
            b: d.b,
            cells: d.cells.into_iter().map(CellDiffDto::from).collect(),
        }
    }
}

impl From<CellDiff> for CellDiffDto {
    fn from(c: CellDiff) -> Self {
        Self {
            index: c.index as u32,
            a: c.a,
            b: c.b,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::load_definition_from_str;

    const BUNDLED_INI: &str = include_str!("../resources/speeduino.sample.ini");

    #[test]
    fn projects_bundled_definition_for_the_ui() {
        let def = load_definition_from_str(BUNDLED_INI).expect("parses");
        let dto = DefinitionDto::from(&def);

        assert_eq!(dto.signature, "speeduino 202504-dev");
        assert_eq!(dto.menus.len(), 1);
        assert_eq!(dto.menus[0].items[0].dialog, "engine_dialog");

        let dialog = &dto.dialogs[0];
        assert_eq!(dialog.name, "engine_dialog");
        assert_eq!(dialog.fields.len(), 3);
        // The gated field carries its raw visibility expression.
        let gated = dialog.fields.last().unwrap();
        assert_eq!(gated.visible.as_deref(), Some("injLayout != 0"));

        // injLayout is a Bits selector with four options.
        let inj = dto
            .constants
            .iter()
            .find(|c| c.name == "injLayout")
            .unwrap();
        match &inj.kind {
            ConstantKindDto::Bits { options } => assert_eq!(options.len(), 4),
            other => panic!("expected Bits, got {other:?}"),
        }
        // reqFuel is a scalar with a literal high bound.
        let req = dto.constants.iter().find(|c| c.name == "reqFuel").unwrap();
        assert_eq!(req.kind, ConstantKindDto::Scalar);
        assert_eq!(req.high, Some(6553.5));
    }

    #[test]
    fn field_diff_dto_narrows_cell_index_to_u32() {
        let diff = FieldDiff {
            name: "map".to_string(),
            a: Value::Array(vec![1.0, 2.0]),
            b: Value::Array(vec![1.0, 9.0]),
            cells: vec![CellDiff {
                index: 1,
                a: 2.0,
                b: 9.0,
            }],
        };
        let dto = FieldDiffDto::from(diff);
        assert_eq!(dto.name, "map");
        assert_eq!(dto.a, Value::Array(vec![1.0, 2.0]));
        assert_eq!(
            dto.cells,
            vec![CellDiffDto {
                index: 1,
                a: 2.0,
                b: 9.0
            }]
        );
    }
}
