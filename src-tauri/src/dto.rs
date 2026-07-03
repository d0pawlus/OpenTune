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
    ConstantDef, ConstantKind, Definition, DialogDef, FieldKind, FrontPageDef, GaugeDef,
    IndicatorDef, MenuDef, Number, TableDef,
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
    /// `[GaugeConfigurations]` entries backing the dashboard (M3).
    pub gauges: Vec<GaugeDto>,
    /// `[FrontPage]` — the default dashboard layout (M3).
    pub frontpage: FrontPageDto,
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

/// A gauge's display rules (title, unit label, thresholds, digits) — the UI
/// projection of [`GaugeDef`]. Numeric bounds follow the [`ConstantDto`]
/// convention: a literal projects to `Some`, an `{ expr }` bound to `None`
/// (the gauge falls back to a neutral zone for `None` thresholds).
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct GaugeDto {
    /// Gauge id referenced by `FrontPageDto::gauge_slots`.
    pub name: String,
    /// The output-channel name it displays (keys the realtime frame map).
    pub channel: String,
    pub title: String,
    /// The unit label shown in the UI (e.g. "RPM", "kPa").
    pub units: String,
    pub low: Option<f64>,
    pub high: Option<f64>,
    pub lo_danger: Option<f64>,
    pub lo_warn: Option<f64>,
    pub hi_warn: Option<f64>,
    pub hi_danger: Option<f64>,
    /// Decimal digits for the live value readout.
    pub value_digits: u32,
    /// Decimal digits for scale labels.
    pub label_digits: u32,
    /// `gaugeCategory`, for grouping menus.
    pub category: String,
}

/// `[FrontPage]` — the default dashboard layout.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct FrontPageDto {
    /// `gauge1..gauge8` → gauge names, in slot order.
    pub gauge_slots: Vec<String>,
    /// Boolean indicator lamps shown alongside the gauges.
    pub indicators: Vec<IndicatorDto>,
}

/// One `[FrontPage]` `indicator` entry (colors are named colors, verbatim).
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct IndicatorDto {
    /// Bare bit-channel name or comparison; evaluated against realtime frames.
    pub expr: String,
    pub off_label: String,
    pub on_label: String,
    pub off_bg: String,
    pub off_fg: String,
    pub on_bg: String,
    pub on_fg: String,
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
            gauges: def.gauges.iter().map(GaugeDto::from).collect(),
            frontpage: FrontPageDto::from(&def.frontpage),
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

impl From<&GaugeDef> for GaugeDto {
    fn from(g: &GaugeDef) -> Self {
        Self {
            name: g.name.clone(),
            channel: g.channel.clone(),
            title: g.title.clone(),
            units: g.units.clone(),
            low: lit(&g.low),
            high: lit(&g.high),
            lo_danger: lit(&g.lo_danger),
            lo_warn: lit(&g.lo_warn),
            hi_warn: lit(&g.hi_warn),
            hi_danger: lit(&g.hi_danger),
            value_digits: u32::from(g.value_digits),
            label_digits: u32::from(g.label_digits),
            category: g.category.clone(),
        }
    }
}

impl From<&FrontPageDef> for FrontPageDto {
    fn from(fp: &FrontPageDef) -> Self {
        Self {
            gauge_slots: fp.gauge_slots.clone(),
            indicators: fp.indicators.iter().map(IndicatorDto::from).collect(),
        }
    }
}

impl From<&IndicatorDef> for IndicatorDto {
    fn from(i: &IndicatorDef) -> Self {
        Self {
            expr: i.expr.clone(),
            off_label: i.off_label.clone(),
            on_label: i.on_label.clone(),
            off_bg: i.off_bg.clone(),
            off_fg: i.off_fg.clone(),
            on_bg: i.on_bg.clone(),
            on_fg: i.on_fg.clone(),
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
    fn bundled_definition_projects_live_gauges_and_frontpage() {
        // Task 8 extended the bundled sample INI with [OutputChannels],
        // [GaugeConfigurations] and [FrontPage] so the default simulator
        // connect drives a non-empty dashboard. The projection must carry
        // them with full referential integrity — and the INI we ship must
        // parse diagnostic-free (no degraded rows).
        let def = load_definition_from_str(BUNDLED_INI).expect("parses");
        let dto = DefinitionDto::from(&def);

        assert!(!dto.gauges.is_empty(), "bundled INI must define gauges");
        assert!(
            !dto.frontpage.gauge_slots.is_empty(),
            "bundled INI must fill front-page gauge slots"
        );
        assert!(
            !dto.frontpage.indicators.is_empty(),
            "bundled INI must define at least one indicator"
        );
        for slot in &dto.frontpage.gauge_slots {
            assert!(
                dto.gauges.iter().any(|g| &g.name == slot),
                "front-page slot `{slot}` must reference a defined gauge"
            );
        }
        for gauge in &dto.gauges {
            assert!(
                def.output_channel(&gauge.channel).is_some(),
                "gauge `{}` references undeclared output channel `{}`",
                gauge.name,
                gauge.channel
            );
        }
        for indicator in &dto.frontpage.indicators {
            assert!(
                def.output_channel(&indicator.expr).is_some(),
                "indicator expr `{}` must be a declared bit channel",
                indicator.expr
            );
        }
        assert!(
            def.diagnostics.is_empty(),
            "the shipped INI must parse without degradation, got {:?}",
            def.diagnostics
        );
    }

    #[test]
    fn gauge_dto_projects_literal_bounds_to_some_and_expr_bounds_to_none() {
        let def = GaugeDef {
            name: "rpmGauge".to_string(),
            channel: "rpm".to_string(),
            title: "Engine Speed".to_string(),
            units: "RPM".to_string(),
            low: Number::Lit(0.0),
            high: Number::Expr("rpmHigh".to_string()),
            lo_danger: Number::Lit(300.0),
            lo_warn: Number::Lit(600.0),
            hi_warn: Number::Lit(6000.0),
            hi_danger: Number::Expr("rpmDanger".to_string()),
            value_digits: 0,
            label_digits: 2,
            category: "Engine".to_string(),
        };
        let dto = GaugeDto::from(&def);
        assert_eq!(dto.name, "rpmGauge");
        assert_eq!(dto.channel, "rpm");
        assert_eq!(dto.title, "Engine Speed");
        assert_eq!(dto.units, "RPM");
        assert_eq!(dto.low, Some(0.0));
        assert_eq!(dto.high, None);
        assert_eq!(dto.lo_danger, Some(300.0));
        assert_eq!(dto.lo_warn, Some(600.0));
        assert_eq!(dto.hi_warn, Some(6000.0));
        assert_eq!(dto.hi_danger, None);
        assert_eq!(dto.value_digits, 0);
        assert_eq!(dto.label_digits, 2);
        assert_eq!(dto.category, "Engine");
    }

    #[test]
    fn front_page_dto_projects_slots_and_indicators() {
        let def = FrontPageDef {
            gauge_slots: vec!["rpmGauge".to_string(), "cltGauge".to_string()],
            indicators: vec![IndicatorDef {
                expr: "running".to_string(),
                off_label: "Not Running".to_string(),
                on_label: "Running".to_string(),
                off_bg: "black".to_string(),
                off_fg: "white".to_string(),
                on_bg: "green".to_string(),
                on_fg: "black".to_string(),
            }],
        };
        let dto = FrontPageDto::from(&def);
        assert_eq!(dto.gauge_slots, vec!["rpmGauge", "cltGauge"]);
        assert_eq!(dto.indicators.len(), 1);
        let ind = &dto.indicators[0];
        assert_eq!(ind.expr, "running");
        assert_eq!(ind.off_label, "Not Running");
        assert_eq!(ind.on_label, "Running");
        assert_eq!(ind.off_bg, "black");
        assert_eq!(ind.off_fg, "white");
        assert_eq!(ind.on_bg, "green");
        assert_eq!(ind.on_fg, "black");
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
