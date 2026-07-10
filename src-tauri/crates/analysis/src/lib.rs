// SPDX-License-Identifier: GPL-3.0-or-later
//! `opentune-analysis` — the deterministic VE (volumetric efficiency) analysis
//! engine (design doc §3.1): given a captured sample set, the current VE
//! table, and its AFR/lambda target table, propose corrected VE cells.
//!
//! **Written fresh, no code port**: TunerStudio/MegaLogViewer's analysis
//! implementation is proprietary (closed source); this crate's algorithm is
//! derived behaviorally from the MegaSquirt "MS Extra" tuning manual and the
//! Speeduino `[VeAnalyze]` binding semantics — math only, no code involved.
//!
//! Deliberately **zero dependencies** (see `Cargo.toml`): the core is pure —
//! same input always produces identical output, no RNG, no `HashMap`
//! iteration, fixed sample order, fixed cell order. This lets it be tested
//! and reasoned about in complete isolation from the `ini`/`model` crates
//! that otherwise touch the same tables.
//!
//! Task 0 froze every type and the [`ve_analyze`] signature with stub
//! bodies; Task 10 implements the real algorithm (Task 11 bridges it to
//! Definition/Tune/SampleSet DTOs).

mod grid;
mod ve_analyze;

pub use grid::TableGrid;
pub use ve_analyze::ve_analyze;

/// Column-oriented capture: channel names pinned once, one f64 row per frame
/// (missing channel = NaN). The owner's ring buffer produces this.
#[derive(Debug, Clone, PartialEq)]
pub struct SampleSet {
    pub columns: Vec<String>,
    /// Per-row ms since capture start (audit only).
    pub t_ms: Vec<f64>,
    /// `rows[i].len() == columns.len()`.
    pub rows: Vec<Vec<f64>>,
}

impl SampleSet {
    /// The column index for a channel name, if captured.
    pub fn column(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c == name)
    }

    /// The number of captured rows (frames).
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether no rows were captured.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// A custom filter's comparison operator — this crate's own copy (zero-dep,
/// no dependency on `opentune-ini`'s `FilterOp`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FilterOp {
    Lt,
    Gt,
    Eq,
    And,
}

/// Which samples to reject, in declaration order (first match wins).
#[derive(Debug, Clone, PartialEq)]
pub enum FilterSpec {
    XAxisMin,
    XAxisMax,
    YAxisMin,
    YAxisMax,
    /// Measured AFR/lambda ≤ 0 (dead sensor).
    DeadLambda,
    /// `channel <op> value` ⇒ rejected. `And` = `(channel as i64 & value as i64) != 0`.
    Custom {
        id: String,
        label: String,
        channel: String,
        op: FilterOp,
        value: f64,
    },
}

/// The channels and filters an analysis run binds against — the
/// `[VeAnalyze]` binding, projected into this crate's own (ini-independent)
/// vocabulary.
#[derive(Debug, Clone, PartialEq)]
pub struct AnalyzeBinding {
    pub x_channel: String,
    pub y_channel: String,
    pub afr_channel: String,
    pub ego_channel: String,
    pub filters: Vec<FilterSpec>,
}

/// Explicit thresholds — every knob is data, none is hidden (§determinism).
#[derive(Debug, Clone, PartialEq)]
pub struct VeAnalyzeParams {
    /// Cells below this summed weight stay unchanged.
    pub min_weight: f64,
    /// Weight where weight-confidence saturates to 1.
    pub confidence_sat_weight: f64,
    /// `confidence /= 1 + penalty * variance`.
    pub variance_penalty: f64,
    /// 0..1 blend toward the current table.
    pub cell_change_resistance: f64,
    /// `|per-cell change|` clamp, % of current.
    pub max_delta_pct: f64,
    /// Wideband delay: `afr[i]` pairs with `point[i - lag]`.
    pub lag_records: u32,
    /// `egoCorrection` no-trim value; 0 disables.
    pub ego_center: f64,
    /// Custom-filter ids to skip.
    pub disabled_filters: Vec<String>,
}

impl Default for VeAnalyzeParams {
    fn default() -> Self {
        Self {
            min_weight: 1.0,
            confidence_sat_weight: 20.0,
            variance_penalty: 4.0,
            cell_change_resistance: 0.2,
            max_delta_pct: 15.0,
            lag_records: 6,
            ego_center: 100.0,
            disabled_filters: Vec::new(),
        }
    }
}

/// One table cell's proposed correction.
#[derive(Debug, Clone, PartialEq)]
pub struct CellResult {
    pub current: f64,
    pub proposed: f64,
    /// `(proposed - current) / current * 100`; 0 when unchanged.
    pub delta_pct: f64,
    /// Summed bilinear weight (MLV "Total Weight").
    pub hit_weight: f64,
    /// Samples whose max-weight cell is this (MLV "Hit Count").
    pub sample_count: u32,
    /// 0..1.
    pub confidence: f64,
}

/// How many samples one filter rejected.
#[derive(Debug, Clone, PartialEq)]
pub struct FilterCount {
    pub id: String,
    pub label: String,
    pub count: u32,
}

/// The result of one [`ve_analyze`] run.
#[derive(Debug, Clone, PartialEq)]
pub struct VeAnalysisReport {
    pub x_len: u32,
    pub y_len: u32,
    /// `len == x_len * y_len`; index `y * x_len + x` (pinned order).
    pub cells: Vec<CellResult>,
    /// Declaration order, built-ins first.
    pub filtered: Vec<FilterCount>,
    pub total_samples: u32,
    pub used_samples: u32,
}

/// Errors [`ve_analyze`] can produce.
#[derive(Debug, Clone, PartialEq)]
pub enum AnalyzeError {
    MissingChannel(String),
    EmptyTable,
    ShapeMismatch(String),
}

// Manual impls (Task 0 review Minor): the crate is zero-dep, so no thiserror.
impl std::fmt::Display for AnalyzeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingChannel(name) => write!(f, "missing channel: {name}"),
            Self::EmptyTable => write!(f, "empty table"),
            Self::ShapeMismatch(msg) => write!(f, "table shape mismatch: {msg}"),
        }
    }
}

impl std::error::Error for AnalyzeError {}
