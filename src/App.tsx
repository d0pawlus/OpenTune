// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useLayoutEffect, useState } from "react";
import { commands, events, type AppInfo } from "./ipc/bindings";
import { useConnectionStore } from "./stores/connection";
import { useRealtimeStore } from "./stores/realtime";
import { Connect } from "./components/Connect";
import { Dashboard } from "./components/dashboard/Dashboard";
import { OfflinePanel } from "./components/offline/OfflinePanel";
import { TunePanel } from "./components/dialogs/TunePanel";
import type { Theme } from "./components/gauges/GaugeCanvas";
import { t, type Locale } from "./i18n";

function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const lastSeq = useConnectionStore((s) => s.lastSeq);
  const [locale, setLocale] = useState<Locale>("en");
  const [theme, setTheme] = useState<Theme>("default");

  useEffect(() => {
    commands.appInfo().then(setInfo);
  }, []);

  useEffect(() => {
    const unlisten = events.heartbeat.listen((e) =>
      useConnectionStore.getState().setSeq(e.payload.seq),
    );
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  useEffect(() => {
    const unlisten = events.connectionStateEvent.listen((e) =>
      useConnectionStore.getState().applyConnectionState(e.payload),
    );
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  // Reflect ≤30 Hz realtime frames into the reflect-only store; canvas
  // gauges read it imperatively (rAF + getState), never via React state.
  useEffect(() => {
    const unlisten = events.realtimeFrameEvent.listen((e) =>
      useRealtimeStore.getState().applyFrame(e.payload),
    );
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  // A layout effect (not a passive one): React flushes *all* layout effects
  // in the tree before *any* passive effect runs, so this attribute is
  // guaranteed to land before descendant canvas gauges' `useEffect`s
  // re-resolve their CSS-token theme off it. With a plain `useEffect` here,
  // child-before-parent passive-effect ordering means a gauge several levels
  // down could re-resolve colors one render *before* this attribute updates,
  // reading the outgoing theme instead of the incoming one.
  useLayoutEffect(() => {
    document.documentElement.dataset.theme =
      theme === "high-contrast" ? "high-contrast" : "";
  }, [theme]);

  const toggleLocale = () => setLocale((prev) => (prev === "en" ? "pl" : "en"));
  const toggleTheme = () =>
    setTheme((prev) =>
      prev === "high-contrast" ? "default" : "high-contrast",
    );

  return (
    <main>
      <h1>{t("app.title", locale)}</h1>
      {info ? `${info.name} v${info.version}` : "…"}
      <p>heartbeat: {lastSeq ?? "—"}</p>

      <Connect locale={locale} />

      <OfflinePanel locale={locale} />

      <Dashboard locale={locale} theme={theme} />

      <TunePanel locale={locale} />

      <div style={{ marginTop: "2rem" }}>
        <button onClick={toggleLocale}>
          {locale === "en" ? "Switch to Polish" : "Przełącz na angielski"}
        </button>
        <button onClick={toggleTheme}>
          {theme === "high-contrast" ? "Default theme" : "High-contrast theme"}
        </button>
      </div>
    </main>
  );
}

export default App;
