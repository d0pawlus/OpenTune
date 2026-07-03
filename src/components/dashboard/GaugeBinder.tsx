// SPDX-License-Identifier: GPL-3.0-or-later
import type { GaugeDto } from "../../ipc/bindings";
import { t, type Locale } from "../../i18n";
import type { GaugeKind, SlotLayout } from "./layout";

const KIND_LABELS = {
  round: "dashboard.kindRound",
  bar: "dashboard.kindBar",
  digital: "dashboard.kindDigital",
} as const satisfies Record<GaugeKind, string>;

interface GaugeBinderProps {
  locale: Locale;
  slot: SlotLayout;
  /** All gauges from the definition to choose from. */
  gauges: GaugeDto[];
  onChange: (next: SlotLayout) => void;
  /** Move this slot one place up (-1) or down (+1). */
  onMove: (delta: -1 | 1) => void;
}

/** Edit-mode controls for one dashboard slot: rebind, restyle, reorder. */
export function GaugeBinder({
  locale,
  slot,
  gauges,
  onChange,
  onMove,
}: GaugeBinderProps) {
  const known = gauges.some((g) => g.name === slot.gauge);
  return (
    <div className="gauge-binder">
      <select
        aria-label={t("dashboard.bindChannel", locale)}
        value={slot.gauge}
        onChange={(e) => onChange({ ...slot, gauge: e.target.value })}
      >
        {/* Keep a stale binding selectable so the control stays consistent. */}
        {!known && <option value={slot.gauge}>{slot.gauge}</option>}
        {gauges.map((g) => (
          <option key={g.name} value={g.name}>
            {g.title || g.name}
          </option>
        ))}
      </select>
      <select
        aria-label={t("dashboard.gaugeKind", locale)}
        value={slot.kind}
        onChange={(e) =>
          onChange({ ...slot, kind: e.target.value as GaugeKind })
        }
      >
        {(Object.keys(KIND_LABELS) as GaugeKind[]).map((kind) => (
          <option key={kind} value={kind}>
            {t(KIND_LABELS[kind], locale)}
          </option>
        ))}
      </select>
      <button
        type="button"
        aria-label={t("dashboard.moveUp", locale)}
        onClick={() => onMove(-1)}
      >
        ↑
      </button>
      <button
        type="button"
        aria-label={t("dashboard.moveDown", locale)}
        onClick={() => onMove(1)}
      >
        ↓
      </button>
    </div>
  );
}
