// SPDX-License-Identifier: GPL-3.0-or-later
import type { CSSProperties } from "react";
import type { Cell, Selection } from "./selection";
import { rectOf } from "./selection";
import { heatColor } from "./heatmap";

export interface TableGridProps {
  /** Prefix for each `<td>`'s id (`${gridId}-${index}`); backs the
   * container's `aria-activedescendant` roving-focus id. */
  gridId: string;
  /** Formatted column-header labels (x bins). */
  xLabels: string[];
  /** Formatted row-header labels, DATA order (this component reverses). */
  yLabels: string[];
  /** Row-major, DATA order; non-finite entries render as "—". */
  values: (number | null)[];
  rows: number;
  cols: number;
  digits: number;
  heatLo: number;
  heatHi: number;
  selection: Selection | null;
  active: Cell | null;
  /** In-progress edit: the cell being typed into and its raw draft text. */
  draft: { cell: Cell; text: string } | null;
  readOnly?: boolean;
  /** AutoTune tooltip hook (Task 11); lands on the `<td title>`. */
  cellTitle?: (index: number) => string | undefined;
  onCellMouseDown: (cell: Cell, shift: boolean) => void;
  onCellMouseEnter: (cell: Cell, buttons: number) => void;
  onDraftChange: (text: string) => void;
}

function isFinite(value: number | null | undefined): value is number {
  return value !== null && value !== undefined && Number.isFinite(value);
}

/**
 * Presentational DOM grid for the table editor: a semantic
 * `<table role="grid">` with `xLabels` as column headers and DATA rows
 * rendered **display-reversed** (top = highest load; display row `d` is data
 * row `rows-1-d` — the tuning convention pinned by the Task 5 brief).
 *
 * No keyboard handling lives here — `TableEditor` owns the single
 * `tabIndex=0` roving-focus surface and passes `active`/`selection`/`draft`
 * down as plain data.
 */
export function TableGrid({
  gridId,
  xLabels,
  yLabels,
  values,
  rows,
  cols,
  digits,
  heatLo,
  heatHi,
  selection,
  active,
  draft,
  readOnly,
  cellTitle,
  onCellMouseDown,
  onCellMouseEnter,
  onDraftChange,
}: TableGridProps) {
  const rect = selection ? rectOf(selection) : null;
  const inSelection = (row: number, col: number): boolean =>
    !!rect &&
    row >= rect.r0 &&
    row <= rect.r1 &&
    col >= rect.c0 &&
    col <= rect.c1;

  const displayRows = Array.from({ length: rows }, (_, d) => rows - 1 - d);

  return (
    <table role="grid" className="te-grid">
      <thead>
        <tr>
          <th scope="col" />
          {xLabels.map((label, c) => (
            <th key={c} scope="col">
              {label}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {displayRows.map((dataRow) => (
          <tr key={dataRow}>
            <th scope="row">{yLabels[dataRow]}</th>
            {Array.from({ length: cols }, (_, col) => {
              const index = dataRow * cols + col;
              const value = values[index];
              const finite = isFinite(value);
              const isActive = active?.row === dataRow && active?.col === col;
              const selected = inSelection(dataRow, col);
              const isDraft =
                draft?.cell.row === dataRow && draft?.cell.col === col;
              const classes = [
                "te-cell",
                selected ? "te-cell--selected" : "",
                isActive ? "te-cell--active" : "",
              ]
                .filter(Boolean)
                .join(" ");
              const style: CSSProperties = finite
                ? { background: heatColor(value, heatLo, heatHi) }
                : {};
              return (
                <td
                  key={col}
                  role="gridcell"
                  id={`${gridId}-${index}`}
                  aria-selected={selected}
                  className={classes}
                  style={style}
                  title={cellTitle?.(index)}
                  onMouseDown={(e) =>
                    onCellMouseDown({ row: dataRow, col }, e.shiftKey)
                  }
                  onMouseEnter={(e) =>
                    onCellMouseEnter({ row: dataRow, col }, e.buttons)
                  }
                >
                  {isDraft ? (
                    <input
                      className="te-cell-input"
                      value={draft.text}
                      onChange={(e) => onDraftChange(e.target.value)}
                      readOnly={readOnly}
                      autoFocus
                    />
                  ) : finite ? (
                    value.toFixed(digits)
                  ) : (
                    "—"
                  )}
                </td>
              );
            })}
          </tr>
        ))}
      </tbody>
    </table>
  );
}
