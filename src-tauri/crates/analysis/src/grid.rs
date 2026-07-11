// SPDX-License-Identifier: GPL-3.0-or-later
//! [`TableGrid`] — a physical-value lookup table with bilinear
//! interpolation — plus the shared [`segment`] primitive that
//! [`crate::ve_analyze`] reuses for its fractional-hit accumulation
//! (design doc §3.1).
//!
//! **Written fresh, no code port** (see the crate-level doc comment):
//! bilinear-interpolation lookup and the VE-correction algorithm are
//! behavioral ports of the MegaSquirt "MS Extra" tuning manual and the
//! Speeduino `[VeAnalyze]` binding — math only.

/// A physical-value table: ascending axis bins + row-major cells
/// (`z[y * x_bins.len() + x]`). Self-contained — no `ini` dependency.
#[derive(Debug, Clone, PartialEq)]
pub struct TableGrid {
    pub x_bins: Vec<f64>,
    pub y_bins: Vec<f64>,
    pub z: Vec<f64>,
}

/// The axis segment containing `v`: `(i, t)` with `bins[i] ≤ v ≤ bins[i+1]`
/// and `t = (v - bins[i]) / (bins[i+1] - bins[i])`. An equal-neighbor
/// (duplicate-bin) segment takes the `t = 0` path instead of dividing by
/// zero. `None` when `v` is non-finite, outside the bins, or the axis has
/// fewer than two bins.
///
/// Deterministic by construction: a fixed forward scan picks the segment
/// (comparisons against a NaN bin are `false`, so the scan just stops —
/// fail-closed, never UB).
pub(crate) fn segment(bins: &[f64], v: f64) -> Option<(usize, f64)> {
    let (first, last) = match (bins.first(), bins.last()) {
        (Some(f), Some(l)) if bins.len() >= 2 => (*f, *l),
        _ => return None,
    };
    if !v.is_finite() || v < first || v > last {
        return None;
    }
    let mut i = 0usize;
    while i + 2 < bins.len() && bins[i + 1] <= v {
        i += 1;
    }
    let span = bins[i + 1] - bins[i];
    let t = if span > 0.0 {
        (v - bins[i]) / span
    } else {
        0.0
    };
    Some((i, t))
}

impl TableGrid {
    /// Bilinear lookup at `(x, y)`; `None` when outside the bins or
    /// shape-invalid (`z.len() != x_bins.len() * y_bins.len()` or an axis
    /// with fewer than two bins).
    pub fn lookup(&self, x: f64, y: f64) -> Option<f64> {
        let x_len = self.x_bins.len();
        if self.z.len() != x_len * self.y_bins.len() {
            return None;
        }
        let (ix, tx) = segment(&self.x_bins, x)?;
        let (iy, ty) = segment(&self.y_bins, y)?;
        let z = |xx: usize, yy: usize| self.z[yy * x_len + xx];
        let lo = z(ix, iy) + (z(ix + 1, iy) - z(ix, iy)) * tx;
        let hi = z(ix, iy + 1) + (z(ix + 1, iy + 1) - z(ix, iy + 1)) * tx;
        Some(lo + (hi - lo) * ty)
    }
}
