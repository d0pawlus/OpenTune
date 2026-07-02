// SPDX-License-Identifier: GPL-3.0-or-later
import { useCallback, useState } from "react";
import { commands } from "../../ipc/bindings";
import type { FieldDiffDto, Value } from "../../ipc/bindings";
import { t, type Locale } from "../../i18n";
import "./diff.css";

/**
 * Pure selection -> merge-command payload mapping: the constant names whose
 * checkbox is checked, in a stable (insertion) order. Extracted so the
 * selection logic is unit-testable without rendering the table.
 */
export function buildMergePayload(
  selection: Record<string, boolean>,
): string[] {
  return Object.entries(selection)
    .filter(([, picked]) => picked)
    .map(([name]) => name);
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
}

/**
 * Task 8 diff/merge panel: snapshot the current tune as a baseline, list the
 * constants that differ from it with a per-row "take" checkbox, and merge
 * the picked constants back — live to the ECU via `merge_tune`. Re-diffs
 * after a successful merge so the table always reflects what's left.
 */
export function TuneDiff({ locale }: TuneDiffProps) {
  const [diffs, setDiffs] = useState<FieldDiffDto[] | null>(null);
  const [selection, setSelection] = useState<Record<string, boolean>>({});
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refreshDiff = useCallback(async () => {
    const res = await commands.diffTune();
    if (res.status === "error") {
      setError(res.error);
      return;
    }
    setDiffs(res.data);
    setSelection({});
  }, []);

  const snapshot = useCallback(async () => {
    setBusy(true);
    setError(null);
    const res = await commands.snapshotTune();
    if (res.status === "error") {
      setError(res.error);
      setBusy(false);
      return;
    }
    await refreshDiff();
    setBusy(false);
  }, [refreshDiff]);

  const toggle = (name: string) =>
    setSelection((s) => ({ ...s, [name]: !s[name] }));

  const mergeSelected = useCallback(async () => {
    const picks = buildMergePayload(selection);
    if (picks.length === 0) return;
    setBusy(true);
    setError(null);
    const res = await commands.mergeTune(picks);
    if (res.status === "error") setError(res.error);
    await refreshDiff();
    setBusy(false);
  }, [selection, refreshDiff]);

  const picked = buildMergePayload(selection).length;

  return (
    <section className="tune-diff" aria-label={t("diff.title", locale)}>
      <header className="tune-diff-header">
        <h3>{t("diff.title", locale)}</h3>
        <div className="tune-diff-actions">
          <button type="button" onClick={snapshot} disabled={busy}>
            {t("diff.snapshot", locale)}
          </button>
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

      {!diffs && (
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
              <tr key={d.name}>
                <td>
                  <input
                    type="checkbox"
                    aria-label={d.name}
                    checked={!!selection[d.name]}
                    onChange={() => toggle(d.name)}
                  />
                </td>
                <td className="tune-diff-name">{d.name}</td>
                <td>{formatValue(d.a)}</td>
                <td>{formatValue(d.b)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}
