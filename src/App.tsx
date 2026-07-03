// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useState } from "react";
import { commands, events, type AppInfo } from "./ipc/bindings";
import { useConnectionStore } from "./stores/connection";
import { useRealtimeStore } from "./stores/realtime";
import { Connect } from "./components/Connect";
import { Dashboard } from "./components/dashboard/Dashboard";
import { TunePanel } from "./components/dialogs/TunePanel";
import { t, type Locale } from "./i18n";

function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const lastSeq = useConnectionStore((s) => s.lastSeq);
  const [locale, setLocale] = useState<Locale>("en");
  const [theme, setTheme] = useState<"default" | "high-contrast">("default");

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

  useEffect(() => {
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

      <Dashboard locale={locale} />

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
