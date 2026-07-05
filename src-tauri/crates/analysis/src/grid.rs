// SPDX-License-Identifier: GPL-3.0-or-later
//! [`TableGrid`] — a physical-value lookup table — and [`ve_analyze`], the
//! deterministic VE analysis engine (design doc §3.1).
//!
//! **Written fresh, no code port** (see the crate-level doc comment):
//! bilinear-interpolation lookup and the VE-correction algorithm are
//! behavioral ports of the MegaSquirt "MS Extra" tuning manual and the
//! Speeduino `[VeAnalyze]` binding — math only. Task 0 freezes the shapes
//! with stub bodies that compile and pin every signature; Task 11 implements
//! the real bilinear lookup and analysis.

use crate::{AnalyzeBinding, AnalyzeError, SampleSet, VeAnalysisReport, VeAnalyzeParams};

/// A physical-value table: ascending axis bins + row-major cells
/// (`z[y * x_bins.len() + x]`). Self-contained — no `ini` dependency.
#[derive(Debug, Clone, PartialEq)]
pub struct TableGrid {
    pub x_bins: Vec<f64>,
    pub y_bins: Vec<f64>,
    pub z: Vec<f64>,
}

impl TableGrid {
    /// Bilinear lookup at `(x, y)`; `None` when outside the bins or
    /// shape-invalid.
    ///
    /// Stub (Task 0): always `None` — pins the signature; Task 11 implements
    /// the real interpolation.
    pub fn lookup(&self, _x: f64, _y: f64) -> Option<f64> {
        None
    }
}

/// THE deterministic engine (design doc §3.1). Pure: same input → identical
/// output. No RNG, no `HashMap` iteration, fixed sample order, fixed cell
/// order.
///
/// Stub (Task 0): always `Err(AnalyzeError::EmptyTable)` — compiles and pins
/// the signature; Task 11 implements the real analysis.
pub fn ve_analyze(
    _samples: &SampleSet,
    _ve: &TableGrid,
    _target: &TableGrid,
    _binding: &AnalyzeBinding,
    _params: &VeAnalyzeParams,
) -> Result<VeAnalysisReport, AnalyzeError> {
    Err(AnalyzeError::EmptyTable)
}
