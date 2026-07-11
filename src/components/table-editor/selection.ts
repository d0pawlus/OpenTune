// SPDX-License-Identifier: GPL-3.0-or-later
// Rectangle-selection arithmetic for the table/curve editors (Tasks 5-7).
// Pure functions only: no DOM, no store, no IPC.

/** A single grid coordinate. */
export type Cell = { row: number; col: number };

/** A rectangular selection expressed as two corners (order not normalized). */
export type Selection = { anchor: Cell; focus: Cell };

/** A normalized, inclusive rectangle: r0 <= r1, c0 <= c1. */
export type Rect = { r0: number; c0: number; r1: number; c1: number };

/** Normalizes a selection's anchor/focus corners into an inclusive rect. */
export function rectOf(sel: Selection): Rect {
  const { anchor, focus } = sel;
  return {
    r0: Math.min(anchor.row, focus.row),
    c0: Math.min(anchor.col, focus.col),
    r1: Math.max(anchor.row, focus.row),
    c1: Math.max(anchor.col, focus.col),
  };
}

/** Clamps a cell to the [0, rows) x [0, cols) grid bounds. */
export function clampCell(cell: Cell, rows: number, cols: number): Cell {
  return {
    row: Math.min(Math.max(cell.row, 0), rows - 1),
    col: Math.min(Math.max(cell.col, 0), cols - 1),
  };
}

/**
 * Moves a selection by (dr, dc), clamped to the grid.
 * When `extend` is false (plain arrow-key move), anchor and focus move
 * together, collapsing to a single cell. When `extend` is true (shift+arrow),
 * only the focus moves and the anchor stays put, growing/shrinking the rect.
 */
export function move(
  sel: Selection,
  dr: number,
  dc: number,
  rows: number,
  cols: number,
  extend: boolean,
): Selection {
  const nextFocus = clampCell(
    { row: sel.focus.row + dr, col: sel.focus.col + dc },
    rows,
    cols,
  );
  if (extend) return { anchor: sel.anchor, focus: nextFocus };
  return { anchor: nextFocus, focus: nextFocus };
}

/** Row-major flat indices (r*cols+c) of every cell inside a rect. */
export function cellIndices(rect: Rect, cols: number): number[] {
  const indices: number[] = [];
  for (let r = rect.r0; r <= rect.r1; r++) {
    for (let c = rect.c0; c <= rect.c1; c++) {
      indices.push(r * cols + c);
    }
  }
  return indices;
}
