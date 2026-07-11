// SPDX-License-Identifier: GPL-3.0-or-later
// Pure geometry/color math for the 3D surface view — testable without WebGL
// (no three.js import here; SurfaceView.tsx is the sole module that touches
// the renderer). Row-major convention throughout, matching tableOps.ts's
// Grid: vertex/cell i = row*cols + col.
import { heatRgb } from "../table-editor/heatmap";

/** A finite min/max extent, as computed by `finiteRange`. */
export interface FiniteRange {
  min: number;
  max: number;
}

/**
 * Finite min/max of `values`, or null when none are finite. Exported (review
 * finding I-1) so SurfaceView's paint loop can be handed a range PRECOMPUTED
 * once on mount/data-change instead of calling this — an O(n) `.filter()` +
 * spread'd `Math.min`/`Math.max` — on every animation frame.
 */
export function finiteRange(values: number[]): FiniteRange | null {
  const finite = values.filter(Number.isFinite);
  if (finite.length === 0) return null;
  return { min: Math.min(...finite), max: Math.max(...finite) };
}

/**
 * Maps `bins` from their finite min..max onto 0..1. A degenerate range (equal
 * min/max, or no finite values) maps every entry to 0.5 — mirrors
 * heatmap.ts's `heatT` ("no gradient to map onto" instead of dividing by 0).
 */
export function normalize(bins: number[]): number[] {
  const range = finiteRange(bins);
  if (!range || range.min >= range.max) return bins.map(() => 0.5);
  const { min, max } = range;
  return bins.map((v) => (Number.isFinite(v) ? (v - min) / (max - min) : 0.5));
}

/**
 * Scalar counterpart of `normalize`: a single physical `value`'s 0..1
 * fraction along `bins`' finite extent. Used for the live dot's x/z, which
 * reads one physical channel reading rather than a whole bin array — kept as
 * the same finite-range formula as `normalize` so the dot always lands in
 * the identical coordinate space as the mesh, not a second computation of
 * "where is this on the axis".
 */
export function axisFraction(value: number, bins: number[]): number {
  return axisFractionIn(finiteRange(bins), value);
}

/**
 * Range-accepting counterpart of `axisFraction` (review finding I-1) — same
 * formula, but takes a `range` PRECOMPUTED by the caller (e.g. via
 * `finiteRange`) instead of re-deriving it from `bins` every call. Lets
 * SurfaceView's paint loop compute the fraction every frame without
 * re-scanning the bins array each time.
 */
export function axisFractionIn(
  range: FiniteRange | null,
  value: number,
): number {
  if (!range || range.min >= range.max) return 0.5;
  return (value - range.min) / (range.max - range.min);
}

/**
 * Scaled height for a single `value` against `values`' own finite extent —
 * the exact mapping `surfacePositions` uses per-vertex, exported so the live
 * dot (SurfaceView's paint loop, fed a bilinearly-interpolated value) computes
 * an y IDENTICAL to the mesh's own vertex heights instead of a second,
 * possibly-diverging height formula. Non-finite input (or an all-non-finite
 * `values`) is height 0, matching `surfacePositions`' per-vertex fallback.
 */
export function heightOf(
  value: number,
  values: number[],
  heightScale: number,
): number {
  return heightOfIn(finiteRange(values), value, heightScale);
}

/**
 * Range-accepting counterpart of `heightOf` (review finding I-1) — same
 * formula, but takes a `range` PRECOMPUTED by the caller (e.g. via
 * `finiteRange`) instead of re-deriving it from `values` every call. Lets
 * SurfaceView's paint loop compute the live dot's height every frame without
 * an O(cells) `.filter()` over the whole values array each time.
 */
export function heightOfIn(
  range: FiniteRange | null,
  value: number,
  heightScale: number,
): number {
  if (!range || range.max <= range.min || !Number.isFinite(value)) return 0;
  return heightScale * ((value - range.min) / (range.max - range.min));
}

/**
 * Vertex positions for a `rows` x `cols` surface grid (row-major: vertex
 * i = row*cols+col). x/z come from the bins normalized to 0..1, so the mesh
 * always spans a unit footprint regardless of the table's physical axis
 * units; y is `heightOf` per cell. A non-finite cell (the backend's NaN
 * sentinel for an unused table cell) gets height 0 rather than an
 * extrapolated fraction.
 */
export function surfacePositions(
  xBins: number[],
  yBins: number[],
  values: number[],
  heightScale: number,
): Float32Array {
  const cols = xBins.length;
  const rows = yBins.length;
  const nx = normalize(xBins);
  const nz = normalize(yBins);
  const positions = new Float32Array(rows * cols * 3);
  for (let r = 0; r < rows; r++) {
    for (let c = 0; c < cols; c++) {
      const i = r * cols + c;
      positions[i * 3] = nx[c];
      positions[i * 3 + 1] = heightOf(values[i], values, heightScale);
      positions[i * 3 + 2] = nz[r];
    }
  }
  return positions;
}

/**
 * Triangle indices for a `rows` x `cols` vertex grid: two CCW triangles per
 * quad, split along the same diagonal (`[v00,v10,v01]`, `[v01,v10,v11]`).
 */
export function surfaceIndices(rows: number, cols: number): Uint32Array {
  const quadCols = cols - 1;
  const quadRows = rows - 1;
  const indices = new Uint32Array(
    Math.max(quadCols, 0) * Math.max(quadRows, 0) * 6,
  );
  let k = 0;
  for (let r = 0; r < quadRows; r++) {
    for (let c = 0; c < quadCols; c++) {
      const v00 = r * cols + c;
      const v01 = v00 + 1;
      const v10 = v00 + cols;
      const v11 = v10 + 1;
      indices[k++] = v00;
      indices[k++] = v10;
      indices[k++] = v01;
      indices[k++] = v01;
      indices[k++] = v10;
      indices[k++] = v11;
    }
  }
  return indices;
}

/**
 * `heatRgb` per vertex, flattened to r,g,b triples — Task 4's blue-to-red
 * scale, the same one the DOM heatmap uses, so the 3D surface and the 2D
 * grid are never two independent color scales. A non-finite value gets a
 * neutral gray instead of an extrapolated hue.
 */
export function surfaceColors(
  values: number[],
  lo: number,
  hi: number,
): Float32Array {
  const colors = new Float32Array(values.length * 3);
  values.forEach((v, i) => {
    const [r, g, b] = Number.isFinite(v) ? heatRgb(v, lo, hi) : [0.5, 0.5, 0.5];
    colors[i * 3] = r;
    colors[i * 3 + 1] = g;
    colors[i * 3 + 2] = b;
  });
  return colors;
}

/** Index of the bin segment bracketing `x` (`bins[i] <= x <= bins[i+1]`,
 * bins assumed ascending — the ECU convention), or null outside the bins'
 * extent. */
function segment(bins: number[], x: number): number | null {
  if (bins.length < 2 || x < bins[0] || x > bins[bins.length - 1]) {
    return null;
  }
  for (let i = 0; i < bins.length - 1; i++) {
    if (x >= bins[i] && x <= bins[i + 1]) return i;
  }
  return null;
}

/**
 * Bilinearly interpolates the table's cell VALUE (not a normalized height)
 * at physical coordinates (x, y) — the live operating-point dot's data
 * source. Returns null outside the bin extents or when a bracketing corner
 * is non-finite, so the dot hides rather than extrapolating or showing a
 * bogus reading.
 */
export function bilinearHeight(
  xBins: number[],
  yBins: number[],
  values: number[],
  x: number,
  y: number,
): number | null {
  const cols = xBins.length;
  const c = segment(xBins, x);
  const r = segment(yBins, y);
  if (c === null || r === null) return null;
  const dx = xBins[c + 1] - xBins[c];
  const dy = yBins[r + 1] - yBins[r];
  const fx = dx === 0 ? 0 : (x - xBins[c]) / dx;
  const fy = dy === 0 ? 0 : (y - yBins[r]) / dy;
  const v00 = values[r * cols + c];
  const v01 = values[r * cols + c + 1];
  const v10 = values[(r + 1) * cols + c];
  const v11 = values[(r + 1) * cols + c + 1];
  // Inline checks, not `[v00,v01,v10,v11].every(Number.isFinite)` (review
  // finding I-1) — this runs every animation frame while the live dot is
  // visible, and the array literal was a per-frame allocation.
  if (
    !Number.isFinite(v00) ||
    !Number.isFinite(v01) ||
    !Number.isFinite(v10) ||
    !Number.isFinite(v11)
  ) {
    return null;
  }
  const top = v00 + (v01 - v00) * fx;
  const bottom = v10 + (v11 - v10) * fx;
  return top + (bottom - top) * fy;
}
