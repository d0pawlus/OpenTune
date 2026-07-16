// SPDX-License-Identifier: GPL-3.0-or-later
//
// 2D heatmap table editor — container. Owns the single keyboard surface
// (roving active cell via aria-activedescendant), the selection/draft state,
// data loading, and every commit path into the store's `setCells`.
//
// Keymap (pinned by the Task 5 brief):
//   arrows move            · Shift+arrows extend the selection
//   Tab / Shift+Tab        · move right / left
//   Ctrl/Cmd+A             · select all
//   Enter                  · edit / commit draft + move down
//   Esc                    · cancel draft, else collapse the selection
//   type [0-9.,-]          · start a draft seeded with the typed char
//   + / -                  · step by 10^-digits (Shift ×10)
//   =                      · set-equal (selection mean)
//   /                      · interpolate (corner-anchored bilinear)
//   s                      · smooth (one 3x3 kernel pass)
//   Ctrl/Cmd+C / V         · copy / paste TSV
//   scale                  · toolbar only (factor input + Apply), no keystroke
//
// Cell edits are NOT link-gated (M3 decision: only burn/undo/redo are
// connected-only; setValue-family commands queue behind a reconnect) —
// mirrors `Field.tsx`.
import { lazy, Suspense, useEffect, useRef, useState } from "react";
import { commands, events } from "../../ipc/bindings";
import type { ConstantDto, TableDto } from "../../ipc/bindings";
import { useTuneStore } from "../../stores/tune";
import { t, type Locale } from "../../i18n";
import type { Cell, Selection } from "./selection";
import { move, rectOf } from "./selection";
import type { CellEdit, Grid } from "./tableOps";
import {
  interpolateRect,
  scaleRect,
  setEqualRect,
  smoothRect,
  stepRect,
} from "./tableOps";
import { parseTsv, pasteEdits, toTsv } from "./tsv";
import { arrayOf, labelsOf, numericOf } from "./binValues";
import { TableGrid } from "./TableGrid";
import { TableToolbar } from "./TableToolbar";
import { AutoTunePanel } from "../autotune/AutoTunePanel";
import { SurfaceErrorBoundary } from "../surface/SurfaceErrorBoundary";
import "./table-editor.css";

// The React.lazy boundary IS the bundle-chunk boundary (locked decision 9):
// SurfaceView (and three, which only it imports) lands in a separate Vite
// chunk, fetched the first time a table is switched to the 3D view. No other
// module may import three or SurfaceView statically.
const LazySurfaceView = lazy(() => import("../surface/SurfaceView"));

/** rows/cols of an Array-kinded constant, or null for any other kind. */
function arrayShape(c: ConstantDto | undefined) {
  const kind = c?.kind;
  if (kind && typeof kind === "object" && "Array" in kind && kind.Array) {
    return kind.Array;
  }
  return null;
}

const DRAFT_START = /^[0-9.,-]$/;
const ORIGIN: Selection = {
  anchor: { row: 0, col: 0 },
  focus: { row: 0, col: 0 },
};
// Stable fallback for the analyze-tables selector: `?? []` inline would mint
// a fresh array per store snapshot and loop useSyncExternalStore forever
// whenever the store holds no definition (embedded-editor contexts).
const NO_ANALYZE_TABLES: string[] = [];

/**
 * Resolves the active table from the store and remounts the editor per table
 * (`key`), so all editor-local state (selection, draft, view, error) resets
 * on switch without reset-state effects.
 */
export function TableEditor({ locale }: { locale: Locale }) {
  const definition = useTuneStore((s) => s.definition);
  const activeTable = useTuneStore((s) => s.activeTable);
  const table =
    definition?.tables.find((tb) => tb.name === activeTable) ?? null;
  if (!table || !definition) return null;
  return (
    <TableEditorView
      key={table.name}
      table={table}
      constants={definition.constants}
      locale={locale}
    />
  );
}

interface EditorProps {
  table: TableDto;
  constants: ConstantDto[];
  locale: Locale;
}

/**
 * The table editor bound to an explicit `table` — also embedded directly by
 * `DialogEngine` when a dialog's `panel =` names a table (rusEFI embeds its
 * VE/ignition tables in dialogs this way).
 */
export function TableEditorView({ table, constants, locale }: EditorProps) {
  const values = useTuneStore((s) => s.values);
  const analyzeTables = useTuneStore(
    (s) => s.definition?.analyze_tables ?? NO_ANALYZE_TABLES,
  );

  const [selection, setSelection] = useState<Selection>(ORIGIN);
  const [draft, setDraft] = useState<{ cell: Cell; text: string } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [view, setView] = useState<"2d" | "3d">("2d");
  const [scaleFactor, setScaleFactor] = useState("1.0");
  // The draft `<input autoFocus>` steals DOM focus from this surface; every
  // draft-closing path refocuses it via `closeDraft` below (M4 final-review
  // fix wave item 6) so the keyboard surface never silently goes dead.
  const surfaceRef = useRef<HTMLDivElement>(null);

  // Fetch-then-merge the three arrays; refetch on every tune_dirty —
  // undo/redo/burn all emit it, keeping the grid honest even after its own
  // optimistic writes.
  useEffect(() => {
    let cancelled = false;
    const names = [table.x_bins, table.y_bins, table.z];
    const fetchValues = async () => {
      const res = await commands.getValues(names);
      if (cancelled || res.status !== "ok") return;
      useTuneStore
        .getState()
        .mergeValues(Object.fromEntries(names.map((n, i) => [n, res.data[i]])));
    };
    fetchValues();
    const unlisten = events.tuneDirtyEvent.listen(fetchValues);
    return () => {
      cancelled = true;
      unlisten.then((f) => f());
    };
  }, [table]);

  const constant = (name: string) => constants.find((c) => c.name === name);
  const zConst = constant(table.z);
  const shape = arrayShape(zConst);
  const zArray = arrayOf(values[table.z]);

  if (!shape || !zArray) {
    return (
      <section className="table-editor" aria-label={table.title || table.name}>
        <h3 className="te-title">{table.title || table.name}</h3>
        <p className="te-empty">{t("table.noValues", locale)}</p>
      </section>
    );
  }

  const { rows, cols } = shape;
  const digits = zConst?.digits ?? 0;
  // null (the backend's NaN sentinel) → NaN so the Task 4 ops skip the cell.
  const gridValues = numericOf(zArray);
  const grid: Grid = { rows, cols, values: gridValues };

  const xBinArray = arrayOf(values[table.x_bins]);
  const yBinArray = arrayOf(values[table.y_bins]);
  const xLabels = labelsOf(xBinArray, constant(table.x_bins)?.digits ?? 0);
  const yLabels = labelsOf(yBinArray, constant(table.y_bins)?.digits ?? 0);

  // Heat range: the z constant's low/high when BOTH are literal (an {expr}
  // bound projects to null), else the finite min/max of the data.
  const heatRange =
    zConst && zConst.low !== null && zConst.high !== null
      ? { lo: zConst.low, hi: zConst.high }
      : null;
  const finiteValues = gridValues.filter((v) => Number.isFinite(v));
  const heatLo = heatRange ? heatRange.lo : Math.min(...finiteValues);
  const heatHi = heatRange ? heatRange.hi : Math.max(...finiteValues);

  const active = selection.focus;
  const rect = rectOf(selection);
  const activeIndex = active.row * cols + active.col;
  const gridId = table.name;
  const activeId = `${gridId}-${activeIndex}`;

  // One commit path for every gesture (typed edit, ops, paste, step).
  const applyEdits = async (edits: CellEdit[]) => {
    if (edits.length === 0) return;
    setError(null);
    try {
      await useTuneStore.getState().setCells(table.z, edits);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  // Closes the open draft and returns keyboard focus to the surface — the
  // draft `<input>` (which stole focus via `autoFocus`) is about to unmount,
  // and nothing else does this (M4 final-review fix wave item 6).
  const closeDraft = (): void => {
    setDraft(null);
    surfaceRef.current?.focus();
  };

  const commitDraft = (): void => {
    if (!draft) return;
    closeDraft();
    // Field.tsx rules: empty/NaN drafts revert, never write. Comma accepted
    // as the PL-locale decimal separator, consistent with parseTsv.
    const text = draft.text.trim();
    const next = Number(text.replace(",", "."));
    if (text === "" || Number.isNaN(next)) return;
    const index = draft.cell.row * cols + draft.cell.col;
    void applyEdits([{ index, value: next }]);
  };

  // The grid renders display-reversed (top = highest data row; see
  // TableGrid), but `toTsv`/`pasteEdits` themselves stay display-agnostic
  // (ascending data-row order — M4 final-review fix wave item 7). Clipboard
  // content must match what the user SEES, so the flip lives here only:
  // reversing `toTsv`'s line order on copy, and un-reversing the parsed rows
  // on paste before handing off to the unchanged `pasteEdits`. A round trip
  // through the same anchor is then an identity operation.
  const copySelection = () => {
    // WKWebView caveat: clipboard access may need a user gesture — copy is
    // always triggered by Ctrl/Cmd+C, which is one; failures degrade to the
    // error line.
    const displayOrderTsv = toTsv(grid, rect, digits)
      .split("\n")
      .reverse()
      .join("\n");
    navigator.clipboard
      .writeText(displayOrderTsv)
      .catch(() => setError(t("table.clipboardError", locale)));
  };

  const pasteClipboard = async () => {
    try {
      const text = await navigator.clipboard.readText();
      const data = parseTsv(text);
      if (!data) return;
      const dataOrder = [...data].reverse();
      const edits = pasteEdits(grid, { row: rect.r0, col: rect.c0 }, dataOrder);
      // Controller policy: paste must not edit non-finite cells (consistent
      // with the five ops). `pasteEdits` is frozen, so filter here on the
      // CURRENT target cell value.
      await applyEdits(
        edits.filter((e) => Number.isFinite(gridValues[e.index])),
      );
    } catch {
      setError(t("table.clipboardError", locale));
    }
  };

  const onKey = (e: React.KeyboardEvent) => {
    const mod = e.ctrlKey || e.metaKey;
    // Y rows display top = highest load, so ArrowUp increases the DATA row.
    const arrows: Record<string, [number, number]> = {
      ArrowUp: [1, 0],
      ArrowDown: [-1, 0],
      ArrowLeft: [0, -1],
      ArrowRight: [0, 1],
    };

    if (e.key in arrows && !mod) {
      e.preventDefault();
      if (draft) commitDraft();
      const [dr, dc] = arrows[e.key];
      setSelection(move(selection, dr, dc, rows, cols, e.shiftKey));
      return;
    }
    if (e.key === "Tab" && !mod) {
      e.preventDefault();
      if (draft) commitDraft();
      setSelection(move(selection, 0, e.shiftKey ? -1 : 1, rows, cols, false));
      return;
    }
    if (mod && (e.key === "a" || e.key === "A")) {
      e.preventDefault();
      setSelection({
        anchor: { row: 0, col: 0 },
        focus: { row: rows - 1, col: cols - 1 },
      });
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      if (draft) {
        commitDraft();
        // "Commit + down": down on screen = a lower data row (display
        // reversal), clamped at the bottom edge.
        setSelection(move(selection, -1, 0, rows, cols, false));
      } else if (Number.isFinite(gridValues[activeIndex])) {
        setDraft({
          cell: active,
          text: gridValues[activeIndex].toFixed(digits),
        });
      }
      return;
    }
    if (e.key === "Escape") {
      e.preventDefault();
      if (draft) closeDraft();
      else setSelection({ anchor: selection.focus, focus: selection.focus });
      return;
    }
    if (mod && (e.key === "c" || e.key === "C")) {
      e.preventDefault();
      copySelection();
      return;
    }
    if (mod && (e.key === "v" || e.key === "V")) {
      e.preventDefault();
      void pasteClipboard();
      return;
    }
    // Everything below acts on cells; while a draft is open (or with a
    // modifier held) let the keystroke fall through to the draft input.
    if (draft || mod) return;

    if (e.key === "+" || e.key === "-") {
      e.preventDefault();
      const step = 10 ** -digits * (e.shiftKey ? 10 : 1);
      void applyEdits(stepRect(grid, rect, e.key === "+" ? step : -step));
      return;
    }
    if (e.key === "=") {
      e.preventDefault();
      void applyEdits(setEqualRect(grid, rect));
      return;
    }
    if (e.key === "/") {
      e.preventDefault();
      void applyEdits(interpolateRect(grid, rect));
      return;
    }
    if (e.key === "s" || e.key === "S") {
      e.preventDefault();
      void applyEdits(smoothRect(grid, rect));
      return;
    }
    if (DRAFT_START.test(e.key)) {
      e.preventDefault();
      if (Number.isFinite(gridValues[activeIndex])) {
        setDraft({ cell: active, text: e.key });
      }
    }
  };

  const onCellMouseDown = (cell: Cell, shift: boolean) => {
    if (draft) commitDraft();
    setSelection((prev) =>
      shift
        ? { anchor: prev.anchor, focus: cell }
        : { anchor: cell, focus: cell },
    );
  };

  const onCellMouseEnter = (cell: Cell, buttons: number) => {
    if (buttons === 1) {
      setSelection((prev) => ({ anchor: prev.anchor, focus: cell }));
    }
  };

  // An empty/unparseable factor must be a NO-OP, never factor 0 (M4
  // final-review fix wave item 5). `Number("")` is 0, not NaN — treat a
  // blank/whitespace-only input as NaN explicitly (same hazard class
  // `commitDraft`'s `text === ""` check already guards against) so
  // `Number.isFinite` alone can gate the button.
  const scaleFactorText = scaleFactor.trim();
  const parsedScaleFactor =
    scaleFactorText === "" ? NaN : Number(scaleFactorText.replace(",", "."));
  const scaleFactorValid = Number.isFinite(parsedScaleFactor);

  const applyScale = () => {
    if (!scaleFactorValid) return;
    void applyEdits(scaleRect(grid, rect, parsedScaleFactor));
  };

  return (
    <section className="table-editor" aria-label={table.title || table.name}>
      <TableToolbar
        locale={locale}
        title={table.title || table.name}
        upDownLabel={table.up_down_label}
        help={table.help}
        view={view}
        scaleFactor={scaleFactor}
        onViewChange={setView}
        onScaleFactorChange={setScaleFactor}
        onInterpolate={() => void applyEdits(interpolateRect(grid, rect))}
        onSmooth={() => void applyEdits(smoothRect(grid, rect))}
        onSetEqual={() => void applyEdits(setEqualRect(grid, rect))}
        onApplyScale={applyScale}
        applyScaleDisabled={!scaleFactorValid}
      />

      {error && <p className="te-error">{error}</p>}

      {view === "3d" ? (
        <div className="te-3d">
          <SurfaceErrorBoundary
            // Not surface.unavailable: the boundary fires on chunk-load and
            // render failures, not on missing WebGL — a wrong "WebGL not
            // supported" diagnosis would send users chasing their GPU.
            fallbackLabel={t("surface.error", locale)}
            retryLabel={t("surface.retry", locale)}
          >
            <Suspense
              fallback={
                <p className="te-3d-loading">{t("surface.loading", locale)}</p>
              }
            >
              <LazySurfaceView
                xBins={numericOf(xBinArray)}
                yBins={numericOf(yBinArray)}
                values={gridValues}
                heatLo={heatLo}
                heatHi={heatHi}
                xChannel={table.x_channel}
                yChannel={table.y_channel}
                unavailableLabel={t("surface.unavailable", locale)}
              />
            </Suspense>
          </SurfaceErrorBoundary>
        </div>
      ) : (
        <div
          ref={surfaceRef}
          className="te-surface"
          tabIndex={0}
          role="application"
          aria-label={table.title || table.name}
          aria-activedescendant={activeId}
          onKeyDown={onKey}
        >
          <TableGrid
            gridId={gridId}
            xLabels={xLabels}
            yLabels={yLabels}
            values={zArray}
            rows={rows}
            cols={cols}
            digits={digits}
            heatLo={heatLo}
            heatHi={heatHi}
            selection={selection}
            active={active}
            draft={draft}
            onCellMouseDown={onCellMouseDown}
            onCellMouseEnter={onCellMouseEnter}
            onDraftChange={(text) => setDraft((d) => (d ? { ...d, text } : d))}
          />
        </div>
      )}

      {analyzeTables.includes(table.name) && (
        <AutoTunePanel
          locale={locale}
          table={table}
          zName={table.z}
          rows={rows}
          cols={cols}
        />
      )}
    </section>
  );
}
