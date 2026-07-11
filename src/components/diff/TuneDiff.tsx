// SPDX-License-Identifier: GPL-3.0-or-later
import { Fragment, useCallback, useState } from "react";
import { commands } from "../../ipc/bindings";
import type {
  FieldDiffDto,
  MergePickDto,
  Value,
} from "../../ipc/bindings";
import { t, type Locale } from "../../i18n";
import "./diff.css";

/**
 * Pure selection -> merge-command payload mapping: the constant names whose
 * checkbox is checked, in a stable (insertion) order. Extracted so the
 * selection logic is unit-testable without rendering the table.
 */
export interface MergeSelection {
  [name: string]: {
    all: boolean;
    cells: Record<number, boolean>;
  };
}

export function buildMergePayload(selection: MergeSelection): MergePickDto[] {
  return Object.entries(selection).flatMap(([name, pick]) => {
    if (pick.all) return [{ type: "all" as const, name }];
    const indices = Object.entries(pick.cells)
      .filter(([, selected]) => selected)
      .map(([index]) => Number(index))
      .filter(Number.isSafeInteger);
    return indices.length > 0
      ? [{ type: "cells" as const, name, indices }]
      : [];
  });
}

/** Render a `Value` compactly for a diff table cell. */
function formatValue(value: Value): string {
  if ("Scalar" in value) return String(value.Scalar ?? 0);
  if ("Enum" in value) return String(value.Enum);
  if ("Text" in value && value.Text) return value.Text;
  if ("Array" in value && value.Array) {
    return value.Array.map((n) => n ?? 0).join(", ");
  }
  return "";
}

interface TuneDiffProps {
  locale: Locale;
  onAfterMerge?: () => Promise<void>;
}

/**
 * Task 8 diff/merge panel. "Snapshot baseline" captures the current tune as
 * the comparison baseline (a one-time capture, not live-updating); "Compare"
 * re-runs the diff against that baseline — separate actions, because the
 * useful flow is snapshot once, edit values elsewhere in the dialog panel
 * above, then compare (possibly repeatedly) before merging. The diff table
 * lists differing constants with a per-row "take" checkbox; "merge
 * selected" writes the picks live to the ECU and re-compares.
 */
export function TuneDiff({
  locale,
  onAfterMerge = async () => {},
}: TuneDiffProps) {
  const [hasSnapshot, setHasSnapshot] = useState(false);
  const [diffs, setDiffs] = useState<FieldDiffDto[] | null>(null);
  const [selection, setSelection] = useState<MergeSelection>({});
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const loadDiff = useCallback(async (clearError: boolean) => {
    if (clearError) setError(null);
    const res = await commands.diffTune();
    if (res.status === "error") {
      setError((current) => current ?? res.error);
      setHasSnapshot(false);
      setDiffs(null);
    } else {
      setDiffs(res.data);
      setSelection({});
    }
  }, []);

  const compare = useCallback(async () => {
    if (busy) return;
    setBusy(true);
    await loadDiff(true);
    setBusy(false);
  }, [busy, loadDiff]);

  const snapshot = useCallback(async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    const res = await commands.snapshotTune();
    if (res.status === "error") {
      setError(res.error);
      setBusy(false);
      return;
    }
    setHasSnapshot(true);
    await loadDiff(false);
    setBusy(false);
  }, [busy, loadDiff]);

  const toggleAll = (name: string) =>
    setSelection((current) => ({
      ...current,
      [name]: {
        all: !current[name]?.all,
        cells: {},
      },
    }));

  const toggleCell = (name: string, index: number) =>
    setSelection((current) => ({
      ...current,
      [name]: {
        all: false,
        cells: {
          ...current[name]?.cells,
          [index]: !current[name]?.cells[index],
        },
      },
    }));

  const mergeSelected = useCallback(async () => {
    if (busy) return;
    const picks = buildMergePayload(selection);
    if (picks.length === 0) return;
    setBusy(true);
    setError(null);
    const res = await commands.mergeTune(picks);
    if (res.status === "error") setError(res.error);
    await onAfterMerge();
    // Refresh the diff without clearing a partial-merge/write error.
    await loadDiff(false);
    setBusy(false);
  }, [busy, selection, onAfterMerge, loadDiff]);

  const picked = buildMergePayload(selection).length;

  return (
    <section className="tune-diff" aria-label={t("diff.title", locale)}>
      <header className="tune-diff-header">
        <h3>{t("diff.title", locale)}</h3>
        <div className="tune-diff-actions">
          <button type="button" onClick={snapshot} disabled={busy}>
            {t("diff.snapshot", locale)}
          </button>
          {hasSnapshot && (
            <button type="button" onClick={compare} disabled={busy}>
              {t("diff.compare", locale)}
            </button>
          )}
          {diffs && diffs.length > 0 && (
            <button
              type="button"
              onClick={mergeSelected}
              disabled={busy || picked === 0}
            >
              {t("diff.mergeSelected", locale)}
            </button>
          )}
        </div>
      </header>

      {error && <p className="tune-error">{error}</p>}

      {!hasSnapshot && (
        <p className="tune-diff-empty">{t("diff.noSnapshot", locale)}</p>
      )}
      {diffs && diffs.length === 0 && (
        <p className="tune-diff-empty">{t("diff.noDifferences", locale)}</p>
      )}

      {diffs && diffs.length > 0 && (
        <table className="tune-diff-table">
          <thead>
            <tr>
              <th>{t("diff.take", locale)}</th>
              <th>{t("diff.constant", locale)}</th>
              <th>{t("diff.current", locale)}</th>
              <th>{t("diff.other", locale)}</th>
            </tr>
          </thead>
          <tbody>
            {diffs.map((d) => (
              <Fragment key={d.name}>
              <tr>
                <td>
                  <input
                    type="checkbox"
                    aria-label={d.name}
                    checked={!!selection[d.name]?.all}
                    onChange={() => toggleAll(d.name)}
                  />
                </td>
                <td className="tune-diff-name">{d.name}</td>
                <td>{formatValue(d.a)}</td>
                <td>{formatValue(d.b)}</td>
              </tr>
              {d.cells.map((cell) => (
                <tr key={`${d.name}-${cell.index}`} className="tune-diff-cell">
                  <td>
                    <input
                      type="checkbox"
                      aria-label={`${d.name}[${cell.index}]`}
                      checked={!!selection[d.name]?.cells[cell.index]}
                      onChange={() => toggleCell(d.name, cell.index)}
                    />
                  </td>
                  <td>
                    {d.name}[{cell.index}]
                  </td>
                  <td>{String(cell.a ?? "—")}</td>
                  <td>{String(cell.b ?? "—")}</td>
                </tr>
              ))}
              </Fragment>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}
