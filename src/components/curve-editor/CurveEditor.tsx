// SPDX-License-Identifier: GPL-3.0-or-later
//
// 1D curve editor — a curve is a `Grid` with `rows: 1` (Task 4 core):
// y-values edit through the SAME selection/ops/TSV/`setCells` machinery as
// `TableEditor` (Task 5); x-bins render as read-only column headers (axis
// editing deferred, see docs/notes/m4-decisions.md). Above the grid: an
// inline SVG preview (redrawn on data change) plus a live cursor line moved
// imperatively via a ref inside a `requestAnimationFrame` loop reading
// `useRealtimeStore` (M3's no-React-state-per-frame rule; see `GaugeCanvas`).
// Keymap is IDENTICAL to `TableEditor`'s (full doc there); `/`/`s` degenerate
// naturally to 1D on a single row. Scale is toolbar-only in Task 5 and this
// task adds no `CurveToolbar`, so it is intentionally unreachable here.
import { useEffect, useRef, useState } from "react";
import { commands, events } from "../../ipc/bindings";
import type { CurveDto, ConstantDto } from "../../ipc/bindings";
import { useTuneStore } from "../../stores/tune";
import { useRealtimeStore } from "../../stores/realtime";
import { t, type Locale } from "../../i18n";
import type { Cell, Selection } from "../table-editor/selection";
import { move, rectOf } from "../table-editor/selection";
import type { CellEdit, Grid } from "../table-editor/tableOps";
import {
  interpolateRect,
  setEqualRect,
  smoothRect,
  stepRect,
} from "../table-editor/tableOps";
import { parseTsv, pasteEdits, toTsv } from "../table-editor/tsv";
import { TableGrid } from "../table-editor/TableGrid";
import { axisRange, polylinePoints, cursorFraction } from "./curveMath";
import { arrayLength, arrayOf, labelsOf, numericOf } from "./binValues";
import "./curve-editor.css";

const PREVIEW_W = 400;
const PREVIEW_H = 200;
const PREVIEW_PAD = 12;

const DRAFT_START = /^[0-9.,-]$/;
const ORIGIN: Selection = {
  anchor: { row: 0, col: 0 },
  focus: { row: 0, col: 0 },
};

/** Resolves the active curve from the store, keyed-remounted per curve. */
export function CurveEditor({ locale }: { locale: Locale }) {
  const definition = useTuneStore((s) => s.definition);
  const activeCurve = useTuneStore((s) => s.activeCurve);
  const curve = definition?.curves.find((c) => c.name === activeCurve) ?? null;
  if (!curve || !definition) return null;
  return (
    <Editor
      key={curve.name}
      curve={curve}
      constants={definition.constants}
      locale={locale}
    />
  );
}

interface EditorProps {
  curve: CurveDto;
  constants: ConstantDto[];
  locale: Locale;
}

function Editor({ curve, constants, locale }: EditorProps) {
  const values = useTuneStore((s) => s.values);
  const [selection, setSelection] = useState<Selection>(ORIGIN);
  const [draft, setDraft] = useState<{ cell: Cell; text: string } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const cursorRef = useRef<SVGLineElement | null>(null);

  // Fetch-then-merge [x_bins, y_bins] on mount + every tune_dirty — the
  // TableEditor 5.5 effect verbatim, with two names instead of three.
  useEffect(() => {
    let cancelled = false;
    const names = [curve.x_bins, curve.y_bins];
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
  }, [curve]);

  const constant = (name: string) => constants.find((c) => c.name === name);
  const yConst = constant(curve.y_bins);
  const cols = arrayLength(yConst);
  const yArray = arrayOf(values[curve.y_bins]);

  // `xr` feeds the rAF effect below, so it must be computed unconditionally,
  // ABOVE the "not loaded yet" early return that follows — every hook must
  // run every render regardless of whether `yArray` has arrived yet.
  const xArray = arrayOf(values[curve.x_bins]);
  const xs = numericOf(xArray);
  const xr = axisRange(curve.x_axis, xs);

  // Imperative rAF cursor (M3 pattern): reads the realtime store directly, no
  // React selector. Depends on `xr`'s scalar bounds, not the `xr` object
  // itself (a fresh reference every render), so it restarts only on a real
  // range change.
  useEffect(() => {
    if (!curve.x_channel) return;
    let frame = 0;
    const paint = () => {
      const v = useRealtimeStore.getState().getChannel(curve.x_channel);
      const el = cursorRef.current;
      if (el) {
        const f = v === undefined ? null : cursorFraction(v, xr);
        if (f === null) {
          el.setAttribute("visibility", "hidden");
        } else {
          const x = PREVIEW_PAD + f * (PREVIEW_W - 2 * PREVIEW_PAD);
          el.setAttribute("visibility", "visible");
          el.setAttribute("x1", String(x));
          el.setAttribute("x2", String(x));
        }
      }
      frame = requestAnimationFrame(paint);
    };
    frame = requestAnimationFrame(paint);
    return () => cancelAnimationFrame(frame);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [curve.x_channel, xr.min, xr.max]);

  if (!cols || !yArray) {
    return (
      <section className="curve-editor" aria-label={curve.title || curve.name}>
        <h3 className="ce-title">{curve.title || curve.name}</h3>
        <p className="ce-empty">{t("table.noValues", locale)}</p>
      </section>
    );
  }

  const digits = yConst?.digits ?? 0;
  const gridValues = numericOf(yArray);
  const grid: Grid = { rows: 1, cols, values: gridValues };

  const xLabels = labelsOf(xArray, constant(curve.x_bins)?.digits ?? 0);
  const yLabels = [""];
  const ys = gridValues;
  const yr = axisRange(curve.y_axis, ys);

  const active = selection.focus;
  const rect = rectOf(selection);
  const activeIndex = active.row * cols + active.col;
  const gridId = curve.name;
  const activeId = `${gridId}-${activeIndex}`;
  const previewPoints = polylinePoints(
    xs,
    ys,
    xr,
    yr,
    PREVIEW_W,
    PREVIEW_H,
    PREVIEW_PAD,
  );

  // One commit path for every gesture — mirrors TableEditor's `applyEdits`,
  // writing through `curve.y_bins` instead of a table's `z`.
  const applyEdits = async (edits: CellEdit[]) => {
    if (edits.length === 0) return;
    setError(null);
    try {
      await useTuneStore.getState().setCells(curve.y_bins, edits);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const commitDraft = (): void => {
    if (!draft) return;
    setDraft(null);
    const text = draft.text.trim();
    const next = Number(text.replace(",", "."));
    if (text === "" || Number.isNaN(next)) return;
    const index = draft.cell.row * cols + draft.cell.col;
    void applyEdits([{ index, value: next }]);
  };

  const copySelection = () => {
    navigator.clipboard
      .writeText(toTsv(grid, rect, digits))
      .catch(() => setError(t("table.clipboardError", locale)));
  };

  const pasteClipboard = async () => {
    try {
      const text = await navigator.clipboard.readText();
      const data = parseTsv(text);
      if (!data) return;
      const edits = pasteEdits(grid, { row: rect.r0, col: rect.c0 }, data);
      await applyEdits(
        edits.filter((e) => Number.isFinite(gridValues[e.index])),
      );
    } catch {
      setError(t("table.clipboardError", locale));
    }
  };

  const onKey = (e: React.KeyboardEvent) => {
    const mod = e.ctrlKey || e.metaKey;
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
      setSelection(move(selection, dr, dc, 1, cols, e.shiftKey));
      return;
    }
    if (e.key === "Tab" && !mod) {
      e.preventDefault();
      if (draft) commitDraft();
      setSelection(move(selection, 0, e.shiftKey ? -1 : 1, 1, cols, false));
      return;
    }
    if (mod && (e.key === "a" || e.key === "A")) {
      e.preventDefault();
      setSelection({
        anchor: { row: 0, col: 0 },
        focus: { row: 0, col: cols - 1 },
      });
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      if (draft) {
        commitDraft();
        setSelection(move(selection, -1, 0, 1, cols, false));
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
      if (draft) setDraft(null);
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

  return (
    <section className="curve-editor" aria-label={curve.title || curve.name}>
      <h3 className="ce-title">{curve.title || curve.name}</h3>
      {error && <p className="ce-error">{error}</p>}

      <figure className="ce-preview" aria-label={t("curve.preview", locale)}>
        <svg
          className="curve-preview"
          viewBox={`0 0 ${PREVIEW_W} ${PREVIEW_H}`}
          role="img"
          aria-label={curve.title || curve.name}
        >
          <polyline
            fill="none"
            stroke="var(--color-accent)"
            strokeWidth="2"
            points={previewPoints}
          />
          <line
            ref={cursorRef}
            className="curve-cursor"
            y1="0"
            y2={PREVIEW_H}
            stroke="var(--color-warn)"
            strokeWidth="1.5"
            visibility="hidden"
          />
        </svg>
        {curve.column_labels.length > 0 && (
          <figcaption className="ce-caption">
            {curve.column_labels.join(" / ")}
          </figcaption>
        )}
      </figure>

      <div
        className="ce-surface"
        tabIndex={0}
        role="application"
        aria-label={curve.title || curve.name}
        aria-activedescendant={activeId}
        onKeyDown={onKey}
      >
        <TableGrid
          gridId={gridId}
          xLabels={xLabels}
          yLabels={yLabels}
          values={yArray}
          rows={1}
          cols={cols}
          digits={digits}
          heatLo={yr.min}
          heatHi={yr.max}
          selection={selection}
          active={active}
          draft={draft}
          onCellMouseDown={onCellMouseDown}
          onCellMouseEnter={onCellMouseEnter}
          onDraftChange={(text) => setDraft((d) => (d ? { ...d, text } : d))}
        />
      </div>
    </section>
  );
}
