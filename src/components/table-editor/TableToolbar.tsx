// SPDX-License-Identifier: GPL-3.0-or-later
import { t, type Locale } from "../../i18n";

export interface TableToolbarProps {
  locale: Locale;
  title: string;
  /** `[up, down]` axis hint from the INI's `upDownLabel`, when present. */
  upDownLabel: string[];
  /** External help URL ("" when the INI names none). */
  help: string;
  view: "2d" | "3d";
  scaleFactor: string;
  onViewChange: (view: "2d" | "3d") => void;
  onScaleFactorChange: (text: string) => void;
  onInterpolate: () => void;
  onSmooth: () => void;
  onSetEqual: () => void;
  onApplyScale: () => void;
  /** Disable Apply when `scaleFactor` doesn't parse to a finite number
   * (empty input included) — never let the button send a factor of 0. */
  applyScaleDisabled: boolean;
}

/**
 * Presentational toolbar for the table editor: title + axis hint, the three
 * selection ops, the scale factor input + Apply (scale runs from here only —
 * no keystroke prompt, per the pinned keymap), the 2D/3D view toggle, and the
 * external help link.
 */
export function TableToolbar({
  locale,
  title,
  upDownLabel,
  help,
  view,
  scaleFactor,
  onViewChange,
  onScaleFactorChange,
  onInterpolate,
  onSmooth,
  onSetEqual,
  onApplyScale,
  applyScaleDisabled,
}: TableToolbarProps) {
  return (
    <header className="te-toolbar">
      <h3 className="te-title">{title}</h3>
      {upDownLabel.length === 2 && (
        <span className="te-hint">
          ↑ {upDownLabel[0]} · ↓ {upDownLabel[1]}
        </span>
      )}
      <div className="te-actions">
        <button type="button" onClick={onInterpolate}>
          {t("table.interpolate", locale)}
        </button>
        <button type="button" onClick={onSmooth}>
          {t("table.smooth", locale)}
        </button>
        <button type="button" onClick={onSetEqual}>
          {t("table.setEqual", locale)}
        </button>
        <label className="te-scale">
          {t("table.scaleFactor", locale)}
          <input
            type="number"
            step="0.05"
            value={scaleFactor}
            onChange={(e) => onScaleFactorChange(e.target.value)}
          />
        </label>
        <button
          type="button"
          onClick={onApplyScale}
          disabled={applyScaleDisabled}
        >
          {t("table.apply", locale)}
        </button>
        <button
          type="button"
          aria-pressed={view === "2d"}
          onClick={() => onViewChange("2d")}
        >
          {t("table.view2d", locale)}
        </button>
        <button
          type="button"
          aria-pressed={view === "3d"}
          onClick={() => onViewChange("3d")}
        >
          {t("table.view3d", locale)}
        </button>
        {help && (
          <a href={help} target="_blank" rel="noreferrer">
            {t("table.help", locale)}
          </a>
        )}
      </div>
    </header>
  );
}
