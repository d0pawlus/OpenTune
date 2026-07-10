// SPDX-License-Identifier: GPL-3.0-or-later
//! [`ve_analyze`] — THE deterministic VE analysis engine (design doc §3.1).
//!
//! **Written fresh, no code port** (see the crate-level doc comment): the
//! correction form (`VE_new = VE_old · AFR_meas / AFR_target`, MS Extra) plus
//! Speeduino `[VeAnalyze]` filter semantics — math only, behavioral
//! references, no proprietary code involved.
//!
//! # The pinned algorithm (M4 Task 10 brief)
//!
//! 1. Validate both grids (axes ≥ 2 bins, `z.len() == x·y`), resolve the
//!    binding channels (`x`/`y`/`afr` required; `ego` only when
//!    `params.ego_center > 0`; custom-filter channels resolve up front — an
//!    absent one is never evaluated but still reports `count: 0`).
//! 2. Lag-pair: the operating point (`x`, `y`, `ego`, custom-filter
//!    channels) comes from `rows[i - lag]`, the measured `afr` from
//!    `rows[i]`.
//! 3. Filter, first match wins. **Evaluation order** (controller-resolved,
//!    pinned by the verbatim suite): `nonFinite` guard first, then
//!    `binding.filters` in declaration order — the `std_*Axis*` filters own
//!    out-of-range rejection — and only then `targetMissing` as the residual
//!    bucket for samples that pass every declared filter yet still can't
//!    pair with a usable target / bin into the grid. The *report* order of
//!    [`FilterCount`] rows stays `[nonFinite, targetMissing,
//!    …binding.filters]`.
//! 4. Per-sample factor: `ego_factor · afr / target`, EGO neutralization
//!    folding the trim the ECU already applied back into the table.
//! 5. Bilinear accumulation (MLV-style fractional hits) into four flat
//!    vectors indexed `y · x_len + x`; `sample_count` on the max-weight cell
//!    only (tie → lowest flat index). Samples accumulate in row order.
//! 6. Per-cell finalize in flat index order: weighted mean/variance →
//!    confidence → resistance blend → ±`max_delta_pct` clamp.
//!
//! Deterministic by construction: no `HashMap`, no RNG, no time; float
//! accumulation order is fully pinned by steps 5–6 — same input is
//! byte-identical output.

use crate::grid::segment;
use crate::{
    AnalyzeBinding, AnalyzeError, CellResult, FilterCount, FilterOp, FilterSpec, SampleSet,
    TableGrid, VeAnalysisReport, VeAnalyzeParams,
};

/// Filter-table index of the built-in `nonFinite` row.
const NON_FINITE: usize = 0;
/// Filter-table index of the built-in `targetMissing` row.
const TARGET_MISSING: usize = 1;
/// Filter-table index of the first `binding.filters` row.
const DECLARED_BASE: usize = 2;

/// THE deterministic engine (design doc §3.1). Pure: same input → identical
/// output. No RNG, no `HashMap` iteration, fixed sample order, fixed cell
/// order.
pub fn ve_analyze(
    samples: &SampleSet,
    ve: &TableGrid,
    target: &TableGrid,
    binding: &AnalyzeBinding,
    params: &VeAnalyzeParams,
) -> Result<VeAnalysisReport, AnalyzeError> {
    validate_grid(ve, "ve")?;
    validate_grid(target, "target")?;
    let cols = resolve_columns(samples, binding, params)?;
    let mut filtered = filter_table(binding);

    let x_len = ve.x_bins.len();
    let mut acc = Accumulators::new(ve.z.len());

    let lag = params.lag_records as usize;
    for i in lag..samples.rows.len() {
        let op_row = &samples.rows[i - lag];
        let meas_row = &samples.rows[i];
        let sample = SampleView {
            x: channel(op_row, cols.x),
            y: channel(op_row, cols.y),
            afr: channel(meas_row, cols.afr),
            ego: cols.ego.map(|c| channel(op_row, c)),
            op_row,
        };

        if let Some(reason) = filter_reason(&sample, ve, binding, params, &cols.custom) {
            filtered[reason].count += 1;
            continue;
        }
        // Residual `targetMissing` bucket: lookup `None`/non-finite/≤ 0, or
        // an operating point that passed every declared filter yet cannot
        // bin into the analysis grid (e.g. no axis filters declared).
        let target_val = match target.lookup(sample.x, sample.y) {
            Some(t) if t.is_finite() && t > 0.0 => t,
            _ => {
                filtered[TARGET_MISSING].count += 1;
                continue;
            }
        };
        let (sx, sy) = match (segment(&ve.x_bins, sample.x), segment(&ve.y_bins, sample.y)) {
            (Some(sx), Some(sy)) => (sx, sy),
            _ => {
                filtered[TARGET_MISSING].count += 1;
                continue;
            }
        };

        let ego_factor = match sample.ego {
            Some(e) => e / params.ego_center,
            None => 1.0,
        };
        let factor = ego_factor * sample.afr / target_val;
        accumulate(sx, sy, x_len, factor, &mut acc);
    }

    let cells: Vec<CellResult> = (0..ve.z.len())
        .map(|idx| {
            finalize(
                ve.z[idx],
                acc.sum_w[idx],
                acc.sum_wf[idx],
                acc.sum_wf2[idx],
                acc.sample_count[idx],
                params,
            )
        })
        .collect();

    let pairs = samples.rows.len().saturating_sub(lag) as u32;
    let rejected: u32 = filtered.iter().map(|f| f.count).sum();
    Ok(VeAnalysisReport {
        x_len: x_len as u32,
        y_len: ve.y_bins.len() as u32,
        cells,
        filtered,
        total_samples: samples.rows.len() as u32,
        used_samples: pairs - rejected,
    })
}

/// The resolved sample-column indices for one run.
struct Columns {
    x: usize,
    y: usize,
    afr: usize,
    /// `Some` only when EGO neutralization is active (`ego_center > 0`).
    ego: Option<usize>,
    /// Per `binding.filters` entry: `Some(col)` for a `Custom` filter whose
    /// channel is captured; `None` for std filters and absent channels
    /// (absent ⇒ the filter is never evaluated but still reported at 0).
    custom: Vec<Option<usize>>,
}

/// One lag-paired sample: operating point from the earlier row, measured
/// `afr` from the later one.
struct SampleView<'a> {
    x: f64,
    y: f64,
    afr: f64,
    ego: Option<f64>,
    op_row: &'a [f64],
}

/// Flat per-cell accumulators indexed `y * x_len + x` — no `HashMap`
/// anywhere (§determinism).
struct Accumulators {
    sum_w: Vec<f64>,
    sum_wf: Vec<f64>,
    sum_wf2: Vec<f64>,
    sample_count: Vec<u32>,
}

impl Accumulators {
    fn new(cells: usize) -> Self {
        Self {
            sum_w: vec![0.0; cells],
            sum_wf: vec![0.0; cells],
            sum_wf2: vec![0.0; cells],
            sample_count: vec![0; cells],
        }
    }
}

/// A channel value; a column beyond the row's width reads as NaN (the
/// crate-level "missing channel = NaN" convention, applied fail-closed).
fn channel(row: &[f64], col: usize) -> f64 {
    row.get(col).copied().unwrap_or(f64::NAN)
}

fn validate_grid(grid: &TableGrid, name: &str) -> Result<(), AnalyzeError> {
    if grid.x_bins.len() < 2 || grid.y_bins.len() < 2 {
        return Err(AnalyzeError::ShapeMismatch(format!(
            "{name}: axes need at least 2 bins each (x: {}, y: {})",
            grid.x_bins.len(),
            grid.y_bins.len()
        )));
    }
    let expected = grid.x_bins.len() * grid.y_bins.len();
    if grid.z.len() != expected {
        return Err(AnalyzeError::ShapeMismatch(format!(
            "{name}: {} cells, expected {expected}",
            grid.z.len()
        )));
    }
    Ok(())
}

fn resolve_columns(
    samples: &SampleSet,
    binding: &AnalyzeBinding,
    params: &VeAnalyzeParams,
) -> Result<Columns, AnalyzeError> {
    let require = |name: &str| {
        samples
            .column(name)
            .ok_or_else(|| AnalyzeError::MissingChannel(name.to_string()))
    };
    let x = require(&binding.x_channel)?;
    let y = require(&binding.y_channel)?;
    let afr = require(&binding.afr_channel)?;
    let ego = if params.ego_center > 0.0 {
        Some(require(&binding.ego_channel)?)
    } else {
        None
    };
    let custom = binding
        .filters
        .iter()
        .map(|spec| match spec {
            FilterSpec::Custom { channel, .. } => samples.column(channel),
            _ => None,
        })
        .collect();
    Ok(Columns {
        x,
        y,
        afr,
        ego,
        custom,
    })
}

/// The pre-built report rows, in the pinned *report* order
/// `[nonFinite, targetMissing, …binding.filters]` — every filter gets a row
/// even at `count: 0` (Task 11's visible-filtering UI).
fn filter_table(binding: &AnalyzeBinding) -> Vec<FilterCount> {
    let row = |id: &str, label: &str| FilterCount {
        id: id.to_string(),
        label: label.to_string(),
        count: 0,
    };
    let mut table = vec![
        row("nonFinite", "Non-finite sample value"),
        row("targetMissing", "Target lookup missing or non-positive"),
    ];
    table.extend(binding.filters.iter().map(|spec| match spec {
        FilterSpec::XAxisMin => row("std_xAxisMin", "X axis minimum"),
        FilterSpec::XAxisMax => row("std_xAxisMax", "X axis maximum"),
        FilterSpec::YAxisMin => row("std_yAxisMin", "Y axis minimum"),
        FilterSpec::YAxisMax => row("std_yAxisMax", "Y axis maximum"),
        FilterSpec::DeadLambda => row("std_DeadLambda", "Dead lambda"),
        FilterSpec::Custom { id, label, .. } => row(id, label),
    }));
    table
}

/// First-match-wins rejection: `nonFinite` guard, then `binding.filters` in
/// declaration order. Returns the index into the [`filter_table`] rows.
/// `targetMissing` (the residual bucket) is attributed by the caller after
/// the target lookup / grid binning actually fail.
fn filter_reason(
    s: &SampleView,
    ve: &TableGrid,
    binding: &AnalyzeBinding,
    params: &VeAnalyzeParams,
    custom_cols: &[Option<usize>],
) -> Option<usize> {
    let non_finite = !s.x.is_finite()
        || !s.y.is_finite()
        || !s.afr.is_finite()
        || s.ego.is_some_and(|e| !e.is_finite());
    if non_finite {
        return Some(NON_FINITE);
    }
    for (k, spec) in binding.filters.iter().enumerate() {
        let rejects = match spec {
            FilterSpec::XAxisMin => s.x < ve.x_bins[0],
            FilterSpec::XAxisMax => s.x > ve.x_bins[ve.x_bins.len() - 1],
            FilterSpec::YAxisMin => s.y < ve.y_bins[0],
            FilterSpec::YAxisMax => s.y > ve.y_bins[ve.y_bins.len() - 1],
            FilterSpec::DeadLambda => s.afr <= 0.0,
            FilterSpec::Custom { id, op, value, .. } => {
                !params.disabled_filters.iter().any(|d| d == id)
                    && custom_rejects(s.op_row, custom_cols[k], *op, *value)
            }
        };
        if rejects {
            return Some(DECLARED_BASE + k);
        }
    }
    None
}

/// A `Custom` filter's predicate; an absent channel is never evaluated and a
/// non-finite channel value never matches (both pinned).
fn custom_rejects(op_row: &[f64], col: Option<usize>, op: FilterOp, value: f64) -> bool {
    let Some(col) = col else {
        return false;
    };
    let v = channel(op_row, col);
    if !v.is_finite() {
        return false;
    }
    match op {
        FilterOp::Lt => v < value,
        FilterOp::Gt => v > value,
        FilterOp::Eq => v == value,
        FilterOp::And => (v as i64) & (value as i64) != 0,
    }
}

/// Bilinear (MLV-style fractional) accumulation: the four cells around the
/// operating point get `w = fx·fy`; `sample_count` goes to the max-weight
/// cell only, ties breaking to the lowest flat index (the loop visits flat
/// indices in ascending order and only a strictly greater weight wins).
fn accumulate(
    (ix, tx): (usize, f64),
    (iy, ty): (usize, f64),
    x_len: usize,
    factor: f64,
    acc: &mut Accumulators,
) {
    let fx = [1.0 - tx, tx];
    let fy = [1.0 - ty, ty];
    let mut best_idx = 0usize;
    let mut best_w = -1.0f64;
    for (dy, wy) in fy.iter().enumerate() {
        for (dx, wx) in fx.iter().enumerate() {
            let w = wx * wy;
            let idx = (iy + dy) * x_len + (ix + dx);
            acc.sum_w[idx] += w;
            acc.sum_wf[idx] += w * factor;
            acc.sum_wf2[idx] += w * factor * factor;
            if w > best_w {
                best_w = w;
                best_idx = idx;
            }
        }
    }
    acc.sample_count[best_idx] += 1;
}

/// One cell's result: below `min_weight` (or a zero current value) the cell
/// is untouched at confidence 0; otherwise weighted mean/variance →
/// confidence → resistance blend → ±`max_delta_pct` clamp.
fn finalize(
    current: f64,
    sum_w: f64,
    sum_wf: f64,
    sum_wf2: f64,
    sample_count: u32,
    params: &VeAnalyzeParams,
) -> CellResult {
    if current == 0.0 || sum_w < params.min_weight {
        return CellResult {
            current,
            proposed: current,
            delta_pct: 0.0,
            hit_weight: sum_w,
            sample_count,
            confidence: 0.0,
        };
    }
    let mean = sum_wf / sum_w;
    let var = (sum_wf2 / sum_w - mean * mean).max(0.0);
    let w_conf = (sum_w / params.confidence_sat_weight).min(1.0);
    let v_conf = 1.0 / (1.0 + params.variance_penalty * var);
    let confidence = w_conf * v_conf;
    let raw = current * mean;
    let blended = current + (raw - current) * confidence * (1.0 - params.cell_change_resistance);
    // `.max(0.0)` guards `clamp`'s `min > max` panic against a (nonsensical)
    // negative `max_delta_pct` — a pure engine must not panic on params.
    let max_delta = (current.abs() * params.max_delta_pct / 100.0).max(0.0);
    let proposed = blended.clamp(current - max_delta, current + max_delta);
    let delta_pct = (proposed - current) / current * 100.0;
    CellResult {
        current,
        proposed,
        delta_pct,
        hit_weight: sum_w,
        sample_count,
        confidence,
    }
}
