// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useState } from "react";
import { commands } from "../../ipc/bindings";
import type { DefinitionDto, GaugeDto } from "../../ipc/bindings";
import { useConnectionStore } from "../../stores/connection";
import { useRealtimeStore } from "../../stores/realtime";
import { useTuneStore } from "../../stores/tune";
import { t, type Locale } from "../../i18n";
import { BarGauge } from "../gauges/BarGauge";
import { DigitalGauge } from "../gauges/DigitalGauge";
import { IndicatorLamp } from "../gauges/IndicatorLamp";
import { RoundGauge } from "../gauges/RoundGauge";
import { GaugeBinder } from "./GaugeBinder";
import {
  defaultSlots,
  moveSlot,
  parseLayout,
  serializeLayout,
  type GaugeKind,
  type SlotLayout,
} from "./layout";
import "../gauges/gauges.css";
import "./dashboard.css";

function SlotGauge({ kind, gauge }: { kind: GaugeKind; gauge: GaugeDto }) {
  if (kind === "bar") return <BarGauge gauge={gauge} />;
  if (kind === "digital") return <DigitalGauge gauge={gauge} />;
  return <RoundGauge gauge={gauge} />;
}

/**
 * The realtime gauge dashboard: renders the INI `[FrontPage]` (or the user's
 * persisted layout) as a grid of canvas gauges plus indicator lamps, with
 * start/stop live controls and an edit mode to rebind, restyle and reorder
 * slots. The layout is persisted as JSON via the `save_layout`/`load_layout`
 * commands (app config dir).
 *
 * The connected panel is a separate component so that disconnecting unmounts
 * it — slot/live/edit state never leaks across connections.
 */
export function Dashboard({ locale }: { locale: Locale }) {
  const connectionState = useConnectionStore((s) => s.connectionState);
  const isConnected = connectionState?.type === "connected";
  const definition = useTuneStore((s) => s.definition);

  if (!isConnected || !definition) {
    return null;
  }
  return <DashboardPanel locale={locale} definition={definition} />;
}

function DashboardPanel({
  locale,
  definition,
}: {
  locale: Locale;
  definition: DefinitionDto;
}) {
  const [slots, setSlots] = useState<SlotLayout[]>([]);
  const [live, setLive] = useState(false);
  const [editing, setEditing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Load the persisted layout on connect; fall back to the INI [FrontPage].
  useEffect(() => {
    let cancelled = false;
    (async () => {
      const res = await commands.loadLayout();
      if (cancelled) return;
      const names = definition.gauges.map((g) => g.name);
      const saved =
        res.status === "ok" && res.data !== null
          ? parseLayout(res.data, names)
          : null;
      setSlots(
        saved && saved.length > 0 ? saved : defaultSlots(definition.frontpage),
      );
    })();
    return () => {
      cancelled = true;
    };
  }, [definition]);

  // Stale channels must not repaint on the next connect (the backend itself
  // stops polling on disconnect) — drop them when the panel unmounts.
  useEffect(() => () => useRealtimeStore.getState().clear(), []);

  const toggleLive = async () => {
    setError(null);
    const res = live
      ? await commands.stopRealtime()
      : await commands.startRealtime();
    if (res.status === "error") {
      setError(res.error);
      return;
    }
    setLive(!live);
  };

  const persistLayout = async () => {
    setError(null);
    const res = await commands.saveLayout(serializeLayout(slots));
    if (res.status === "error") {
      setError(res.error);
      return;
    }
    setEditing(false);
  };

  const gaugesByName = new Map(definition.gauges.map((g) => [g.name, g]));
  const { indicators } = definition.frontpage;

  return (
    <section className="dashboard" aria-label={t("dashboard.title", locale)}>
      <header className="dashboard-header">
        <h2>{t("dashboard.title", locale)}</h2>
        <div className="dashboard-actions">
          <button type="button" onClick={toggleLive} aria-pressed={live}>
            {live
              ? t("dashboard.stopLive", locale)
              : t("dashboard.startLive", locale)}
          </button>
          {editing ? (
            <button type="button" onClick={persistLayout}>
              {t("dashboard.saveLayout", locale)}
            </button>
          ) : (
            <button
              type="button"
              onClick={() => setEditing(true)}
              disabled={slots.length === 0}
            >
              {t("dashboard.editLayout", locale)}
            </button>
          )}
        </div>
      </header>

      {error && <p className="dashboard-error">{error}</p>}

      {slots.length === 0 ? (
        <p className="dashboard-empty">{t("dashboard.noGauges", locale)}</p>
      ) : (
        <div className="dashboard-grid">
          {slots.map((slot, i) => {
            const gauge = gaugesByName.get(slot.gauge);
            return (
              <div className="dashboard-slot" key={`${i}-${slot.gauge}`}>
                {gauge ? (
                  <SlotGauge kind={slot.kind} gauge={gauge} />
                ) : (
                  // Fail-open per item: a slot bound to a gauge the INI no
                  // longer defines renders neutral, never crashes the panel.
                  <div
                    className="dashboard-slot-missing"
                    role="img"
                    aria-label={slot.gauge}
                  >
                    —
                  </div>
                )}
                {editing && (
                  <GaugeBinder
                    locale={locale}
                    slot={slot}
                    gauges={definition.gauges}
                    onChange={(next) =>
                      setSlots((prev) =>
                        prev.map((s, j) => (j === i ? next : s)),
                      )
                    }
                    onMove={(delta) =>
                      setSlots((prev) => moveSlot(prev, i, delta))
                    }
                  />
                )}
              </div>
            );
          })}
        </div>
      )}

      {indicators.length > 0 && (
        <div className="dashboard-indicators">
          {indicators.map((indicator, i) => (
            <IndicatorLamp
              key={`${i}-${indicator.expr}`}
              indicator={indicator}
            />
          ))}
        </div>
      )}
    </section>
  );
}
