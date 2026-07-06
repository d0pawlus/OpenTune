// SPDX-License-Identifier: GPL-3.0-or-later
// TSV clipboard round-trip for the table/curve editors: export a rect to
// tab/newline-separated text, parse pasted text back into a numeric grid,
// and clip a pasted block to the destination grid's bounds.
// Pure functions only; no DOM (no navigator.clipboard) and no store/IPC.

import type { Rect, Cell } from "./selection";
import type { Grid, CellEdit } from "./tableOps";

/** Serializes `rect` of `g` as TSV: rows joined by "\n", cells by "\t". */
export function toTsv(g: Grid, rect: Rect, digits: number): string {
  const lines: string[] = [];
  for (let r = rect.r0; r <= rect.r1; r++) {
    const cells: string[] = [];
    for (let c = rect.c0; c <= rect.c1; c++) {
      const v = g.values[r * g.cols + c];
      cells.push(Number.isFinite(v) ? v.toFixed(digits) : "");
    }
    lines.push(cells.join("\t"));
  }
  return lines.join("\n");
}

/**
 * Parses TSV text into a numeric grid, or `null` if any cell is not a
 * number. Accepts PL-locale comma decimals ("1,5" -> 1.5). A single trailing
 * blank line (a common paste artifact) is dropped before parsing.
 */
export function parseTsv(text: string): number[][] | null {
  const lines = text.split(/\r?\n/);
  if (lines.length > 1 && lines[lines.length - 1] === "") lines.pop();
  const rows: number[][] = [];
  for (const line of lines) {
    const row: number[] = [];
    for (const cell of line.split("\t")) {
      const n = Number(cell.trim().replace(",", "."));
      if (Number.isNaN(n)) return null;
      row.push(n);
    }
    rows.push(row);
  }
  return rows;
}

/** Builds cell edits from pasting `data` at `at`, clipped to `g`'s bounds. */
export function pasteEdits(g: Grid, at: Cell, data: number[][]): CellEdit[] {
  const edits: CellEdit[] = [];
  for (let r = 0; r < data.length; r++) {
    const targetRow = at.row + r;
    if (targetRow < 0 || targetRow >= g.rows) continue;
    const row = data[r];
    for (let c = 0; c < row.length; c++) {
      const targetCol = at.col + c;
      if (targetCol < 0 || targetCol >= g.cols) continue;
      edits.push({ index: targetRow * g.cols + targetCol, value: row[c] });
    }
  }
  return edits;
}
