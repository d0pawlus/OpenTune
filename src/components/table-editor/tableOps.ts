// SPDX-License-Identifier: GPL-3.0-or-later
// Table-editing operations: interpolate, smooth, scale, set-equal, step.
// Pure functions over a row-major Grid; no DOM, no store, no IPC.
//
// WRITE FRESH (ADR-0006): hypertuner-cloud (MIT) is a read-only viewer with no
// ops to lift; LibreTune (GPL-2) is study-only. Semantics below are pinned by
// the Task 4 brief, not ported from either project:
//   - interpolate = corner-anchored bilinear. The four rect corners are kept;
//     everything else in the rect is recomputed from them. A 1xN/Nx1 rect
//     degenerates to linear interpolation; a single-cell rect is a no-op.
//   - smooth = one pass of a 3x3 kernel (center 4, edge 2, corner 1), window
//     clipped at the grid bounds and renormalized by the surviving weight.
//     Neighbors are read from the whole grid; writes stay inside the rect.
//   - set-equal default = arithmetic mean of the finite selected values.
//   - non-finite cells (the backend's NaN -> null IPC sentinel) are never
//     edited and never contribute to any computation (mean, corners, kernel).

import type { Rect } from "./selection";
import { cellIndices } from "./selection";

/** Row-major numeric grid: values[r*cols+c]. */
export type Grid = { rows: number; cols: number; values: number[] };

/** One cell write: a flat row-major index plus its new value. */
export type CellEdit = { index: number; value: number };

const idx = (g: Grid, r: number, c: number): number => r * g.cols + c;
const at = (g: Grid, r: number, c: number): number => g.values[idx(g, r, c)];
const editable = (v: number): boolean => Number.isFinite(v);

/**
 * Bilinearly (or linearly, for a 1xN/Nx1 rect) fills the interior of `rect`
 * from its four corners. Corners are left untouched. A single-cell rect and a
 * rect whose corners include a non-finite value are both no-ops.
 */
export function interpolateRect(g: Grid, rect: Rect): CellEdit[] {
  const h = rect.r1 - rect.r0;
  const w = rect.c1 - rect.c0;
  if (h === 0 && w === 0) return [];
  const corners = [
    at(g, rect.r0, rect.c0),
    at(g, rect.r0, rect.c1),
    at(g, rect.r1, rect.c0),
    at(g, rect.r1, rect.c1),
  ];
  if (corners.some((v) => !editable(v))) return [];
  const [tl, tr, bl, br] = corners;
  const edits: CellEdit[] = [];
  for (let r = rect.r0; r <= rect.r1; r++) {
    for (let c = rect.c0; c <= rect.c1; c++) {
      const isCorner =
        (r === rect.r0 || r === rect.r1) && (c === rect.c0 || c === rect.c1);
      if (isCorner || !editable(at(g, r, c))) continue;
      const fr = h === 0 ? 0 : (r - rect.r0) / h;
      const fc = w === 0 ? 0 : (c - rect.c0) / w;
      const top = tl + (tr - tl) * fc;
      const bottom = bl + (br - bl) * fc;
      edits.push({ index: idx(g, r, c), value: top + (bottom - top) * fr });
    }
  }
  return edits;
}

// Flattened 3x3 kernel, row-major over (dr, dc) in [-1, 1]: center 4, edge 2, corner 1.
const SMOOTH_KERNEL = [1, 2, 1, 2, 4, 2, 1, 2, 1];

/**
 * One pass of a 3x3 smoothing kernel. The window is clipped at the grid
 * bounds and renormalized by the surviving weight; non-finite neighbors (and
 * the center cell itself, when non-finite) are skipped and never written.
 */
export function smoothRect(g: Grid, rect: Rect): CellEdit[] {
  const edits: CellEdit[] = [];
  for (let r = rect.r0; r <= rect.r1; r++) {
    for (let c = rect.c0; c <= rect.c1; c++) {
      if (!editable(at(g, r, c))) continue;
      let sum = 0;
      let weight = 0;
      for (let dr = -1; dr <= 1; dr++) {
        for (let dc = -1; dc <= 1; dc++) {
          const rr = r + dr;
          const cc = c + dc;
          if (rr < 0 || rr >= g.rows || cc < 0 || cc >= g.cols) continue;
          const v = at(g, rr, cc);
          if (!editable(v)) continue;
          const k = SMOOTH_KERNEL[(dr + 1) * 3 + (dc + 1)];
          sum += v * k;
          weight += k;
        }
      }
      if (weight > 0) edits.push({ index: idx(g, r, c), value: sum / weight });
    }
  }
  return edits;
}

/** Multiplies every finite selected cell by `factor`. */
export function scaleRect(g: Grid, rect: Rect, factor: number): CellEdit[] {
  return cellIndices(rect, g.cols)
    .filter((i) => editable(g.values[i]))
    .map((i) => ({ index: i, value: g.values[i] * factor }));
}

/**
 * Sets every finite selected cell to `value`, or (by default) to the
 * arithmetic mean of the finite selected values. Non-finite cells are never
 * edited and never contribute to the mean.
 */
export function setEqualRect(g: Grid, rect: Rect, value?: number): CellEdit[] {
  const indices = cellIndices(rect, g.cols).filter((i) =>
    editable(g.values[i]),
  );
  if (indices.length === 0) return [];
  const target =
    value !== undefined
      ? value
      : indices.reduce((sum, i) => sum + g.values[i], 0) / indices.length;
  return indices.map((i) => ({ index: i, value: target }));
}

/** Adds `delta` to every finite selected cell. */
export function stepRect(g: Grid, rect: Rect, delta: number): CellEdit[] {
  return cellIndices(rect, g.cols)
    .filter((i) => editable(g.values[i]))
    .map((i) => ({ index: i, value: g.values[i] + delta }));
}
