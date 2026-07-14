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
    ConstantDef, ConstantKind, CurveAxis, CurveDef, Definition, DialogDef, FieldKind, FrontPageDef,
    GaugeDef, IndicatorDef, MenuDef, Number, TableDef,
};
use opentune_model::{CellDiff, FieldDiff, MergePick, Value};

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
    /// Curve (2-D) editors (M4).
    pub curves: Vec<CurveDto>,
    /// `[GaugeConfigurations]` entries backing the dashboard (M3).
    pub gauges: Vec<GaugeDto>,
    /// `[FrontPage]` — the default dashboard layout (M3).
    pub frontpage: FrontPageDto,
    /// `[TableEditor]` ids that carry a `[VeAnalyze]` map — the frontend's
    /// "show AutoTune here" signal (M4 Task 11).
    pub analyze_tables: Vec<String>,
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
    pub title: String,
    pub page: u32,
    pub x_bins: String,
    /// Output channel driving the live X cursor ("" when the INI names none).
    pub x_channel: String,
    pub y_bins: String,
    pub y_channel: String,
    pub z: String,
    pub xy_labels: Vec<String>,
    pub up_down_label: Vec<String>,
    pub help: String,
}

/// Curve axis bounds when literal (an `{expr}` bound resolves to `None` —
/// the frontend falls back to data extents).
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct AxisDto {
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub divisions: u32,
}

/// A curve editor definition (M4).
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct CurveDto {
    pub name: String,
    pub title: String,
    pub column_labels: Vec<String>,
    pub x_axis: Option<AxisDto>,
    pub y_axis: Option<AxisDto>,
    pub x_bins: String,
    pub x_channel: String,
    pub y_bins: String,
    pub gauge: String,
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

/// Gauge bounds resolved against the currently loaded tune.
///
/// Each bound fails open independently: `None` means the INI expression
/// could not be resolved and geometry using that bound must render neutral.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct ResolvedGaugeBoundsDto {
    pub name: String,
    pub low: Option<f64>,
    pub high: Option<f64>,
    pub lo_danger: Option<f64>,
    pub lo_warn: Option<f64>,
    pub hi_warn: Option<f64>,
    pub hi_danger: Option<f64>,
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
            curves: def.curves.iter().map(CurveDto::from).collect(),
            gauges: def.gauges.iter().map(GaugeDto::from).collect(),
            frontpage: FrontPageDto::from(&def.frontpage),
            analyze_tables: def
                .ve_analyze
                .iter()
                .flat_map(|v| v.maps.iter().map(|m| m.table.clone()))
                .collect(),
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
            title: t.title.clone(),
            page: t.page,
            x_bins: t.x_bins.clone(),
            x_channel: t.x_channel.clone(),
            y_bins: t.y_bins.clone(),
            y_channel: t.y_channel.clone(),
            z: t.z.clone(),
            xy_labels: t.xy_labels.clone(),
            up_down_label: t.up_down_label.clone(),
            help: t.help.clone(),
        }
    }
}

impl From<&CurveDef> for CurveDto {
    fn from(c: &CurveDef) -> Self {
        Self {
            name: c.name.clone(),
            title: c.title.clone(),
            column_labels: c.column_labels.clone(),
            x_axis: c.x_axis.as_ref().map(AxisDto::from),
            y_axis: c.y_axis.as_ref().map(AxisDto::from),
            x_bins: c.x_bins.clone(),
            x_channel: c.x_channel.clone(),
            y_bins: c.y_bins.clone(),
            gauge: c.gauge.clone(),
        }
    }
}

impl From<&CurveAxis> for AxisDto {
    fn from(a: &CurveAxis) -> Self {
        Self {
            min: lit(&a.min),
            max: lit(&a.max),
            divisions: a.divisions,
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

/// A field-level or cell-level selective merge request from the frontend.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum MergePickDto {
    All { name: String },
    Cells { name: String, indices: Vec<u32> },
}

impl From<MergePickDto> for MergePick {
    fn from(value: MergePickDto) -> Self {
        match value {
            MergePickDto::All { name } => Self::All(name),
            MergePickDto::Cells { name, indices } => Self::Cells {
                name,
                indices: indices.into_iter().map(|index| index as usize).collect(),
            },
        }
    }
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

// ── M4: table cell edits / capture / VE analysis (Task 0 seam, Tasks 3/8/11) ─

/// One flat cell edit for [`crate::owner::Command::SetCells`] — command
/// *input*, hence `Deserialize` in addition to the usual `Serialize`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct CellEditDto {
    pub index: u32,
    pub value: f64,
}

/// The realtime-capture ring buffer's status (Task 8).
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct CaptureStatusDto {
    pub capturing: bool,
    pub sample_count: u32,
    pub duration_ms: f64,
    pub dropped: u32,
}

/// IPC projection of `opentune_analysis::CellResult`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct CellResultDto {
    pub current: f64,
    pub proposed: f64,
    pub delta_pct: f64,
    pub hit_weight: f64,
    pub sample_count: u32,
    pub confidence: f64,
}

impl From<opentune_analysis::CellResult> for CellResultDto {
    fn from(c: opentune_analysis::CellResult) -> Self {
        Self {
            current: c.current,
            proposed: c.proposed,
            delta_pct: c.delta_pct,
            hit_weight: c.hit_weight,
            sample_count: c.sample_count,
            confidence: c.confidence,
        }
    }
}

/// IPC projection of `opentune_analysis::FilterCount`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct FilterCountDto {
    pub id: String,
    pub label: String,
    pub count: u32,
}

impl From<opentune_analysis::FilterCount> for FilterCountDto {
    fn from(f: opentune_analysis::FilterCount) -> Self {
        Self {
            id: f.id,
            label: f.label,
            count: f.count,
        }
    }
}

/// IPC projection of `opentune_analysis::VeAnalysisReport` (the `table` field
/// is the bridge's own addition — the report itself doesn't name the table
/// it corrects, since the engine is table-agnostic).
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct VeAnalysisReportDto {
    pub table: String,
    pub x_len: u32,
    pub y_len: u32,
    pub cells: Vec<CellResultDto>,
    pub filtered: Vec<FilterCountDto>,
    pub total_samples: u32,
    pub used_samples: u32,
}

// ── M5: datalog and deterministic log analysis ─────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum LogFormatDto {
    Csv,
    MlgV1,
}

impl From<LogFormatDto> for opentune_datalog::LogFormat {
    fn from(value: LogFormatDto) -> Self {
        match value {
            LogFormatDto::Csv => Self::Csv,
            LogFormatDto::MlgV1 => Self::MlgV1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct LogSummaryDto {
    /// The generation token for this `opened_log` assignment (M5 review
    /// CRITICAL — C2): every later command reading `opened_log`
    /// (`get_log_data`, `save_log`, `log_stats`, `detect_anomaly`,
    /// `virtual_dyno`) must echo this id back, and is rejected once a later
    /// `open_log`/`stop_log` has superseded it.
    pub log_id: u32,
    pub fields: Vec<LogFieldDto>,
    pub record_count: u32,
    pub marker_count: u32,
    pub duration_ms: f64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct LogFieldDto {
    pub name: String,
    pub units: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct MarkerDto {
    pub record_index: u32,
    pub t_ms: f64,
    pub text: String,
}

/// Bounded columnar transfer: no object allocation per data point.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct LogDataDto {
    pub offset: u32,
    pub total_records: u32,
    pub t_ms: Vec<f64>,
    /// Field-major columns; non-finite/missing values become `null`.
    pub columns: Vec<Vec<Option<f64>>>,
    pub markers: Vec<MarkerDto>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct LogStatusDto {
    pub active: bool,
    pub path: Option<String>,
    pub format: Option<LogFormatDto>,
    pub record_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum ComparisonDto {
    LessThan,
    LessOrEqual,
    GreaterThan,
    GreaterOrEqual,
    Equal,
    NotEqual,
}

impl From<ComparisonDto> for opentune_analysis::Comparison {
    fn from(value: ComparisonDto) -> Self {
        match value {
            ComparisonDto::LessThan => Self::LessThan,
            ComparisonDto::LessOrEqual => Self::LessOrEqual,
            ComparisonDto::GreaterThan => Self::GreaterThan,
            ComparisonDto::GreaterOrEqual => Self::GreaterOrEqual,
            ComparisonDto::Equal => Self::Equal,
            ComparisonDto::NotEqual => Self::NotEqual,
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct SampleFilterDto {
    pub channel: String,
    pub comparison: ComparisonDto,
    pub value: f64,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct LogStatsParamsDto {
    pub channels: Vec<String>,
    pub reject_when: Vec<SampleFilterDto>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct SummaryStatDto {
    pub channel: String,
    pub finite_count: u32,
    pub missing_count: u32,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub mean: Option<f64>,
    pub std_dev: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct ReasonCountDto {
    pub reason: String,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct FilterDecisionDto {
    pub row: u32,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct LogStatsReportDto {
    pub total_rows: u32,
    pub accepted_rows: u32,
    pub stats: Vec<SummaryStatDto>,
    pub filtered: Vec<ReasonCountDto>,
    pub decisions: Vec<FilterDecisionDto>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct SensorThresholdDto {
    pub channel: String,
    pub min: f64,
    pub max: f64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct AnomalyThresholdsDto {
    pub sensors: Vec<SensorThresholdDto>,
    pub afr_channel: String,
    pub lean_afr: f64,
    pub lean_min_rpm: f64,
    pub rpm_channel: String,
    pub load_channel: String,
    pub lean_min_load: f64,
    pub knock_channel: String,
    pub knock_threshold: f64,
    pub knock_min_rpm: f64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub enum AnomalyKindDto {
    SensorDropout,
    LeanSpike,
    Knock,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct AnomalyDto {
    pub row: u32,
    pub t_ms: f64,
    pub kind: AnomalyKindDto,
    pub channel: String,
    pub value: Option<f64>,
    pub threshold: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct AnomalyReportDto {
    pub inspected_rows: u32,
    pub anomalies: Vec<AnomalyDto>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct VirtualDynoParamsDto {
    pub speed_channel: String,
    pub rpm_channel: String,
    pub mass_kg: f64,
    pub drag_coefficient: f64,
    pub frontal_area_m2: f64,
    pub rolling_resistance: f64,
    pub drivetrain_loss: f64,
    pub smoothing_window: u32,
    pub air_density_kg_m3: f64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct DynoPointDto {
    pub row: u32,
    pub t_ms: f64,
    pub speed_m_s: f64,
    pub rpm: f64,
    pub acceleration_m_s2: f64,
    pub inertial_force_n: f64,
    pub aero_force_n: f64,
    pub rolling_force_n: f64,
    pub wheel_power_w: f64,
    pub wheel_hp: f64,
    pub estimated_engine_power_w: f64,
    pub estimated_engine_hp: f64,
    pub estimated_engine_torque_nm: f64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct DynoConditionDto {
    pub row: u32,
    pub accepted: bool,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct VirtualDynoReportDto {
    pub points: Vec<DynoPointDto>,
    pub conditions: Vec<DynoConditionDto>,
    pub assumptions: Vec<String>,
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
        // The gated field's single trailing `{ cond }` is the *enable*
        // condition (third position); visibility is the fourth position and is
        // absent here. Conditions are positional per the TunerStudio grammar.
        let gated = dialog.fields.last().unwrap();
        assert_eq!(gated.enable.as_deref(), Some("injLayout != 0"));
        assert_eq!(gated.visible, None);

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
    fn bundled_definition_projects_its_ve_analyze_map_as_analyze_tables() {
        // The Task 9 bundled INI declares one `[VeAnalyze]` map, correcting
        // `veTable1Tbl` — the frontend's "show AutoTune here" signal.
        let def = load_definition_from_str(BUNDLED_INI).expect("parses");
        let dto = DefinitionDto::from(&def);
        assert_eq!(dto.analyze_tables, vec!["veTable1Tbl".to_string()]);
    }

    #[test]
    fn cell_result_dto_and_filter_count_dto_project_field_for_field() {
        let cell = opentune_analysis::CellResult {
            current: 50.0,
            proposed: 54.0,
            delta_pct: 8.0,
            hit_weight: 24.0,
            sample_count: 24,
            confidence: 1.0,
        };
        let dto = CellResultDto::from(cell);
        assert_eq!(dto.current, 50.0);
        assert_eq!(dto.proposed, 54.0);
        assert_eq!(dto.delta_pct, 8.0);
        assert_eq!(dto.hit_weight, 24.0);
        assert_eq!(dto.sample_count, 24);
        assert_eq!(dto.confidence, 1.0);

        let filter = opentune_analysis::FilterCount {
            id: "minCltFilter".to_string(),
            label: "Minimum CLT".to_string(),
            count: 3,
        };
        let dto = FilterCountDto::from(filter);
        assert_eq!(dto.id, "minCltFilter");
        assert_eq!(dto.label, "Minimum CLT");
        assert_eq!(dto.count, 3);
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
