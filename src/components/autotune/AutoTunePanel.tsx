// SPDX-License-Identifier: GPL-3.0-or-later
//
// AutoTune panel (M4 Task 11) — roadmap's "auto-tune with visible filtering
// + confidence". Mounted at the bottom of a [TableEditor] whose table carries
// a [VeAnalyze] map (`definition.analyze_tables`). Owns its own capture
// controls (start/stop, 1 Hz status poll) and the analyze/apply gesture; the
// table editor above it owns no autotune state.
//
// Wire note: `specta-typescript` 0.0.12 projects every backend `f64` as
// `number | null` (its NaN-safety convention, already established for
// CaptureStatusDto/CellDiffDto/etc). `VeAnalysisReportDto`'s per-cell floats
// follow the same shape even though the engine never actually emits null —
// `num()` below applies the same `?? 0` fallback already used elsewhere
// (`TuneDiff.tsx`'s `formatValue`) for arithmetic/formatting, while the
// TableGrid `values` array is passed through un-coerced so a genuinely
// non-finite cell still renders "—" (the non-finite convention shared by
// every table/curve editor).
import { useEffect, useState } from "react";
import { commands } from "../../ipc/bindings";
import type {
  CaptureStatusDto,
  TableDto,
  VeAnalysisReportDto,
} from "../../ipc/bindings";
import { useTuneStore } from "../../stores/tune";
import { t, type Locale } from "../../i18n";
import { TableGrid } from "../table-editor/TableGrid";
import "./autotune.css";

const DEFAULT_THRESHOLD = 0.5;
const STATUS_POLL_MS = 1000;

interface AutoTunePanelProps {
  locale: Locale;
  table: TableDto;
  zName: string;
  rows: number;
  cols: number;
}

const num = (v: number | null): number => v ?? 0;
const noop = () => {};

export function AutoTunePanel({
  locale,
  table,
  zName,
  rows,
  cols,
}: AutoTunePanelProps) {
  const [status, setStatus] = useState<CaptureStatusDto | null>(null);
  const [report, setReport] = useState<VeAnalysisReportDto | null>(null);
  const [threshold, setThreshold] = useState(DEFAULT_THRESHOLD);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // 1 Hz status poll while capturing — a UI refresh, not a hot path.
  // Cleared on unmount and whenever capturing flips off.
  useEffect(() => {
    if (!status?.capturing) return;
    const id = setInterval(() => {
      void commands.captureStatus().then((res) => {
        if (res.status === "ok") setStatus(res.data);
      });
    }, STATUS_POLL_MS);
    return () => clearInterval(id);
  }, [status?.capturing]);

  const refreshStatus = async () => {
    const res = await commands.captureStatus();
    if (res.status === "ok") setStatus(res.data);
  };

  const startCapture = async () => {
    setError(null);
    const res = await commands.startCapture();
    if (res.status === "error") {
      setError(res.error);
      return;
    }
    await refreshStatus();
  };

  const stopCapture = async () => {
    setError(null);
    const res = await commands.stopCapture();
    if (res.status === "error") {
      setError(res.error);
      return;
    }
    setStatus(res.data);
  };

  const analyze = async () => {
    setError(null);
    setBusy(true);
    try {
      const res = await commands.runVeAnalyze(table.name);
      if (res.status === "error") {
        setError(res.error);
        return;
      }
      setReport(res.data);
    } finally {
      setBusy(false);
    }
  };

  const apply = () => {
    if (!report) return;
    const edits = report.cells.flatMap((c, i) => {
      const proposed = num(c.proposed);
      const current = num(c.current);
      return num(c.confidence) >= threshold && proposed !== current
        ? [{ index: i, value: proposed }]
        : [];
    });
    if (edits.length === 0) return;
    void useTuneStore.getState().setCells(zName, edits);
  };

  const cellTitle = (i: number): string | undefined => {
    const c = report?.cells[i];
    if (!c) return undefined;
    return (
      `${num(c.current).toFixed(1)} → ${num(c.proposed).toFixed(1)} · ` +
      `conf ${num(c.confidence).toFixed(2)} · w ${num(c.hit_weight).toFixed(1)} · n ${c.sample_count}`
    );
  };

  const maxAbs = report
    ? Math.max(1, ...report.cells.map((c) => Math.abs(num(c.delta_pct))))
    : 1;
  const indexLabels = (n: number) =>
    Array.from({ length: n }, (_, i) => String(i));

  return (
    <section className="autotune" aria-label={t("autotune.title", locale)}>
      <h3 className="at-title">{t("autotune.title", locale)}</h3>

      <div className="at-controls">
        <button type="button" onClick={() => void startCapture()}>
          {t("autotune.startCapture", locale)}
        </button>
        <button type="button" onClick={() => void stopCapture()}>
          {t("autotune.stopCapture", locale)}
        </button>
        <span className="at-samples">
          {t("autotune.samples", locale)}: {status?.sample_count ?? 0}
        </span>
        <button type="button" onClick={() => void analyze()} disabled={busy}>
          {t("autotune.analyze", locale)}
        </button>
      </div>

      {error && <p className="at-error">{error}</p>}

      {report ? (
        <>
          <div className="at-grid">
            <TableGrid
              gridId="autotune"
              xLabels={indexLabels(cols)}
              yLabels={indexLabels(rows)}
              values={report.cells.map((c) => c.delta_pct)}
              rows={rows}
              cols={cols}
              digits={1}
              heatLo={-maxAbs}
              heatHi={maxAbs}
              selection={null}
              active={null}
              draft={null}
              readOnly
              cellTitle={cellTitle}
              onCellMouseDown={noop}
              onCellMouseEnter={noop}
              onDraftChange={noop}
            />
          </div>

          <h4 className="at-subtitle">{t("autotune.filtered", locale)}</h4>
          <ul className="at-filtered">
            {report.filtered.map((f) => (
              <li key={f.id}>{`${f.label} — ${f.count}`}</li>
            ))}
          </ul>

          <div className="at-apply">
            <label className="at-threshold">
              {t("autotune.threshold", locale)}
              <input
                type="number"
                min={0}
                max={1}
                step={0.05}
                value={threshold}
                onChange={(e) => setThreshold(Number(e.target.value))}
              />
            </label>
            <button type="button" onClick={apply}>
              {t("autotune.apply", locale)}
            </button>
          </div>
        </>
      ) : (
        <p className="at-empty">{t("autotune.noReport", locale)}</p>
      )}
    </section>
  );
}
