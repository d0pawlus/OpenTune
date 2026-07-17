// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useLayoutEffect, useState } from "react";
import { commands, events, type AppInfo } from "./ipc/bindings";
import { useConnectionStore } from "./stores/connection";
import { useDatalogStore } from "./stores/datalog";
import { useRealtimeStore } from "./stores/realtime";
import { Connect } from "./components/Connect";
import { Dashboard } from "./components/dashboard/Dashboard";
import { OfflinePanel } from "./components/offline/OfflinePanel";
import { TunePanel } from "./components/dialogs/TunePanel";
import { DatalogPanel } from "./components/datalog/DatalogPanel";
import { Onboarding } from "./components/onboarding/Onboarding";
import { UpdateNotice } from "./components/update/UpdateNotice";
import type { Theme } from "./components/gauges/GaugeCanvas";
import { t, type Locale } from "./i18n";
import {
  completeOnboarding,
  initialLocale,
  initialTheme,
  isOnboardingComplete,
  saveLocale,
  saveTheme,
} from "./preferences";

function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [infoFailed, setInfoFailed] = useState(false);
  const lastSeq = useConnectionStore((s) => s.lastSeq);
  const [locale, setLocale] = useState<Locale>(initialLocale);
  const [theme, setTheme] = useState<Theme>(initialTheme);
  const [onboardingOpen, setOnboardingOpen] = useState(
    () => !isOnboardingComplete(),
  );

  useEffect(() => {
    commands
      .appInfo()
      .then(setInfo)
      .catch((e: unknown) => {
        console.error("Failed to load app info", e);
        setInfoFailed(true);
      });
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
    const unlisten = events.realtimeFrameEvent.listen((e) => {
      if (!useDatalogStore.getState().replaying) {
        useRealtimeStore.getState().applyFrame(e.payload);
      }
    });
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

  const changeLocale = (next: Locale) => {
    setLocale(next);
    saveLocale(next);
  };
  const changeTheme = (next: Theme) => {
    setTheme(next);
    saveTheme(next);
  };
  const toggleLocale = () => changeLocale(locale === "en" ? "pl" : "en");
  const toggleTheme = () =>
    changeTheme(theme === "high-contrast" ? "default" : "high-contrast");
  const finishOnboarding = () => {
    completeOnboarding();
    setOnboardingOpen(false);
  };

  return (
    <>
      <main inert={onboardingOpen ? true : undefined}>
        <h1>{t("app.title", locale)}</h1>
        {info ? `${info.name} v${info.version}` : infoFailed ? "—" : "…"}
        <p>
          {t("app.heartbeat", locale)}: {lastSeq ?? "—"}
        </p>

        <UpdateNotice locale={locale} />

        <Connect locale={locale} />

        <OfflinePanel locale={locale} />

        <Dashboard locale={locale} theme={theme} />

        <TunePanel locale={locale} />

        <DatalogPanel locale={locale} />

        <footer className="app-footer" aria-label={t("app.controls", locale)}>
          <button type="button" onClick={toggleLocale}>
            {locale === "en"
              ? t("app.switchToPolish", locale)
              : t("app.switchToEnglish", locale)}
          </button>
          <button type="button" onClick={toggleTheme}>
            {theme === "high-contrast"
              ? t("theme.default", locale)
              : t("theme.highContrast", locale)}
          </button>
          <button type="button" onClick={() => setOnboardingOpen(true)}>
            {t("onboarding.reopen", locale)}
          </button>
        </footer>
      </main>
      <Onboarding
        open={onboardingOpen}
        locale={locale}
        theme={theme}
        onLocaleChange={changeLocale}
        onThemeChange={changeTheme}
        onComplete={finishOnboarding}
      />
    </>
  );
}

export default App;
