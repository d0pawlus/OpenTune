// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useRef, useState } from "react";
import { commands } from "../../ipc/bindings";
import type { DefinitionDto, GaugeDto } from "../../ipc/bindings";
import { isLinkAlive, useConnectionStore } from "../../stores/connection";
import { useRealtimeStore } from "../../stores/realtime";
import { useTuneStore } from "../../stores/tune";
import { t, type Locale } from "../../i18n";
import { BarGauge } from "../gauges/BarGauge";
import { DigitalGauge } from "../gauges/DigitalGauge";
import type { Theme } from "../gauges/GaugeCanvas";
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

function SlotGauge({
  kind,
  gauge,
  theme,
}: {
  kind: GaugeKind;
  gauge: GaugeDto;
  theme: Theme;
}) {
  if (kind === "bar") return <BarGauge gauge={gauge} theme={theme} />;
  if (kind === "digital") return <DigitalGauge gauge={gauge} theme={theme} />;
  return <RoundGauge gauge={gauge} theme={theme} />;
}

/**
 * The realtime gauge dashboard: renders the INI `[FrontPage]` (or the user's
 * persisted layout) as a grid of canvas gauges plus indicator lamps, with
 * start/stop live controls and an edit mode to rebind, restyle and reorder
 * slots. The layout is persisted as JSON via the `save_layout`/`load_layout`
 * commands (app config dir).
 *
 * The panel is a separate component that stays mounted while the link is
 * alive per {@link isLinkAlive} (`connected` or `reconnecting`) — the backend
 * deliberately keeps realtime polling armed through a link drop, so
 * live/edit state and unsaved layout edits must survive a glitch (gauges
 * keep showing the last received values). Only the terminal states
 * (`disconnected`/`failed`) unmount it, so slot/live/edit state never leaks
 * across connections.
 *
 * `TunePanel` shares the same `useTuneStore`-held `definition` and gates its
 * own reset-on-disconnect logic with the same {@link isLinkAlive} predicate
 * (rather than a stricter connected-only check) precisely so its store reset
 * never fires mid-glitch and unmounts this panel out from under a live demo.
 */
export function Dashboard({ locale, theme }: { locale: Locale; theme: Theme }) {
  const connectionState = useConnectionStore((s) => s.connectionState);
  const linkAlive = isLinkAlive(connectionState);
  const definition = useTuneStore((s) => s.definition);

  if (!linkAlive || !definition) {
    return null;
  }
  return (
    <DashboardPanel locale={locale} theme={theme} definition={definition} />
  );
}

function DashboardPanel({
  locale,
  theme,
  definition,
}: {
  locale: Locale;
  theme: Theme;
  definition: DefinitionDto;
}) {
  const isConnected = useConnectionStore(
    (s) => s.connectionState?.type === "connected",
  );
  const [slots, setSlots] = useState<SlotLayout[]>([]);
  const [loadedDefinition, setLoadedDefinition] =
    useState<DefinitionDto | null>(null);
  const [live, setLive] = useState(false);
  const [editing, setEditing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [realtimePending, setRealtimePending] = useState(false);
  const [savePending, setSavePending] = useState(false);
  const realtimePendingRef = useRef(false);
  const savePendingRef = useRef(false);
  const layoutLoading = loadedDefinition !== definition;

  // Load the persisted layout on connect; fall back to the INI [FrontPage].
  useEffect(() => {
    let cancelled = false;
    (async () => {
      let next = defaultSlots(definition.frontpage);
      try {
        const res = await commands.loadLayout();
        const names = definition.gauges.map((g) => g.name);
        const saved =
          res.status === "ok" && res.data !== null
            ? parseLayout(res.data, names)
            : null;
        if (saved && saved.length > 0) next = saved;
      } catch {
        // An invocation failure is equivalent to no usable persisted layout.
      }
      if (cancelled) return;
      setSlots(next);
      setLoadedDefinition(definition);
    })();
    return () => {
      cancelled = true;
    };
  }, [definition]);

  // Stale channels must not repaint on the next connect (the backend itself
  // stops polling on disconnect) — drop them when the panel unmounts.
  useEffect(() => () => useRealtimeStore.getState().clear(), []);

  const toggleLive = async () => {
    if (realtimePendingRef.current) return;
    realtimePendingRef.current = true;
    setRealtimePending(true);
    setError(null);
    try {
      const res = live
        ? await commands.stopRealtime()
        : await commands.startRealtime();
      if (res.status === "error") {
        setError(res.error);
        return;
      }
      setLive(!live);
    } finally {
      realtimePendingRef.current = false;
      setRealtimePending(false);
    }
  };

  const persistLayout = async () => {
    if (layoutLoading || savePendingRef.current) return;
    savePendingRef.current = true;
    setSavePending(true);
    setError(null);
    try {
      const res = await commands.saveLayout(serializeLayout(slots));
      if (res.status === "error") {
        setError(res.error);
        return;
      }
      setEditing(false);
    } finally {
      savePendingRef.current = false;
      setSavePending(false);
    }
  };

  const gaugesByName = new Map(definition.gauges.map((g) => [g.name, g]));
  const { indicators } = definition.frontpage;

  return (
    <section
      className="dashboard"
      aria-label={t("dashboard.title", locale)}
      aria-busy={layoutLoading}
    >
      <header className="dashboard-header">
        <h2>{t("dashboard.title", locale)}</h2>
        <div className="dashboard-actions">
          <button
            type="button"
            onClick={toggleLive}
            aria-pressed={live}
            // Starting realtime touches the wire → strict `connected` only
            // (same gate as TunePanel's wire buttons; a click mid-reconnect
            // would arm polling against a dead link). Stopping only disarms
            // and stays available through a glitch.
            disabled={realtimePending || (!live && !isConnected)}
          >
            {live
              ? t("dashboard.stopLive", locale)
              : t("dashboard.startLive", locale)}
          </button>
          {editing ? (
            <button
              type="button"
              onClick={persistLayout}
              disabled={layoutLoading || savePending}
            >
              {t("dashboard.saveLayout", locale)}
            </button>
          ) : (
            <button
              type="button"
              onClick={() => setEditing(true)}
              disabled={layoutLoading || slots.length === 0}
            >
              {t("dashboard.editLayout", locale)}
            </button>
          )}
        </div>
      </header>

      {error && <p className="dashboard-error">{error}</p>}

      {!layoutLoading && slots.length === 0 ? (
        <p className="dashboard-empty">{t("dashboard.noGauges", locale)}</p>
      ) : !layoutLoading ? (
        <div className="dashboard-grid">
          {slots.map((slot, i) => {
            const gauge = gaugesByName.get(slot.gauge);
            return (
              <div className="dashboard-slot" key={`${i}-${slot.gauge}`}>
                {gauge ? (
                  <SlotGauge kind={slot.kind} gauge={gauge} theme={theme} />
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
      ) : null}

      {indicators.length > 0 && (
        <div className="dashboard-indicators">
          {indicators.map((indicator, i) => (
            <IndicatorLamp
              key={`${i}-${indicator.expr}`}
              indicator={indicator}
              theme={theme}
            />
          ))}
        </div>
      )}
    </section>
  );
}
