// SPDX-License-Identifier: GPL-3.0-or-later
// Pure curve math for the 1D curve editor's SVG preview: axis range
// resolution, coordinate mapping for the polyline, and live-cursor
// positioning. No DOM, no store, no IPC. WRITE FRESH (ADR-0006, as Task 4's
// selection/ops/tsv/heatmap) — exact signatures/behaviors pinned by the
// Task 6 brief.

/** An inclusive numeric range (an axis's display bounds). */
export type Range = { min: number; max: number };

/**
 * Resolves a curve axis's display range. A literal `AxisDto` bound (BOTH
 * `min` and `max` non-null) wins outright; a single missing bound is
 * treated the same as both missing (the brief's "bounds are null", plural)
 * — falls back to the finite extent of `data`. When neither source yields a
 * range (no literal bounds AND no finite data point), returns the neutral
 * `{min: 0, max: 1}` default rather than dividing by zero downstream.
 */
export function axisRange(
  axis: { min: number | null; max: number | null } | null | undefined,
  data: number[],
): Range {
  if (axis && axis.min !== null && axis.max !== null) {
    return { min: axis.min, max: axis.max };
  }
  const finite = data.filter((v) => Number.isFinite(v));
  if (finite.length === 0) return { min: 0, max: 1 };
  return { min: Math.min(...finite), max: Math.max(...finite) };
}

/**
 * Linear fraction of `v` within `[r.min, r.max]`; `0.5` when the range is
 * degenerate (`max <= min`) — mirrors `heatmap.ts`'s `heatT` zero-division
 * guard, so a flat axis renders at mid-scale instead of NaN/Infinity.
 */
function fractionOf(v: number, r: Range): number {
  return r.max > r.min ? (v - r.min) / (r.max - r.min) : 0.5;
}

/**
 * Maps `(xs, ys)` through `(xr, yr)` into an SVG `points` attribute string
 * (`"x1,y1 x2,y2 ..."`), inside a `w`x`h` viewport padded by `pad` on every
 * side. Y is inverted (data-up renders screen-up: a higher `y` value sits
 * nearer the top). A pair is skipped entirely when either its `x` or `y` is
 * non-finite — the uniform non-finite policy ("never edited, never
 * contribute") applied to the preview.
 */
export function polylinePoints(
  xs: number[],
  ys: number[],
  xr: Range,
  yr: Range,
  w: number,
  h: number,
  pad: number,
): string {
  const innerW = w - 2 * pad;
  const innerH = h - 2 * pad;
  const points: string[] = [];
  const n = Math.min(xs.length, ys.length);
  for (let i = 0; i < n; i++) {
    const x = xs[i];
    const y = ys[i];
    if (!Number.isFinite(x) || !Number.isFinite(y)) continue;
    const screenX = pad + fractionOf(x, xr) * innerW;
    const screenY = h - pad - fractionOf(y, yr) * innerH;
    points.push(`${screenX},${screenY}`);
  }
  return points.join(" ");
}

/**
 * Fraction (0..1) of `xValue` within `xr`, or `null` when `xValue` is
 * non-finite, outside `[xr.min, xr.max]`, or `xr` is degenerate
 * (`max <= min`) — the live cursor hides rather than pin to an edge or
 * divide by zero.
 */
export function cursorFraction(xValue: number, xr: Range): number | null {
  if (!Number.isFinite(xValue) || xr.max <= xr.min) return null;
  if (xValue < xr.min || xValue > xr.max) return null;
  return (xValue - xr.min) / (xr.max - xr.min);
}
