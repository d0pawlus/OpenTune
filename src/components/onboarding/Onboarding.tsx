// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useRef } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { Theme } from "../gauges/GaugeCanvas";
import { t, type Locale } from "../../i18n";
import "./onboarding.css";

const QUICK_START_URL = "https://d0pawlus.github.io/OpenTune/quick-start/";
const FOCUSABLE =
  'button:not([disabled]), a[href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

interface OnboardingProps {
  open: boolean;
  locale: Locale;
  theme: Theme;
  onLocaleChange: (locale: Locale) => void;
  onThemeChange: (theme: Theme) => void;
  onComplete: () => void;
}

export function Onboarding({
  open,
  locale,
  theme,
  onLocaleChange,
  onThemeChange,
  onComplete,
}: OnboardingProps) {
  const dialogRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const previous = document.activeElement as HTMLElement | null;
    const first = dialogRef.current?.querySelector<HTMLElement>(FOCUSABLE);
    first?.focus();
    return () => previous?.focus();
  }, [open]);

  if (!open) return null;

  const onKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      onComplete();
      return;
    }
    if (event.key !== "Tab") return;

    const focusable = Array.from(
      dialogRef.current?.querySelectorAll<HTMLElement>(FOCUSABLE) ?? [],
    );
    if (focusable.length === 0) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  };

  return (
    <div className="onboarding-backdrop">
      <div
        ref={dialogRef}
        className="onboarding-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="onboarding-title"
        onKeyDown={onKeyDown}
      >
        <h2 id="onboarding-title">{t("onboarding.title", locale)}</h2>
        <p>{t("onboarding.intro", locale)}</p>

        <div
          className="onboarding-choice"
          role="group"
          aria-label={t("onboarding.language", locale)}
        >
          <button
            type="button"
            aria-pressed={locale === "en"}
            onClick={() => onLocaleChange("en")}
          >
            English
          </button>
          <button
            type="button"
            aria-pressed={locale === "pl"}
            onClick={() => onLocaleChange("pl")}
          >
            Polski
          </button>
        </div>

        <div
          className="onboarding-choice"
          role="group"
          aria-label={t("onboarding.theme", locale)}
        >
          <button
            type="button"
            aria-pressed={theme === "default"}
            onClick={() => onThemeChange("default")}
          >
            {t("onboarding.themeDefault", locale)}
          </button>
          <button
            type="button"
            aria-pressed={theme === "high-contrast"}
            onClick={() => onThemeChange("high-contrast")}
          >
            {t("onboarding.themeContrast", locale)}
          </button>
        </div>

        <div className="onboarding-workflows">
          <section>
            <h3>{t("onboarding.simulatorTitle", locale)}</h3>
            <p>{t("onboarding.simulatorBody", locale)}</p>
          </section>
          <section>
            <h3>{t("onboarding.offlineTitle", locale)}</h3>
            <p>{t("onboarding.offlineBody", locale)}</p>
          </section>
          <section>
            <h3>{t("onboarding.hardwareTitle", locale)}</h3>
            <p>{t("onboarding.hardwareBody", locale)}</p>
          </section>
        </div>

        <div className="onboarding-actions">
          <button type="button" onClick={() => void openUrl(QUICK_START_URL)}>
            {t("onboarding.quickStart", locale)}
          </button>
          <button type="button" onClick={onComplete}>
            {t("onboarding.complete", locale)}
          </button>
        </div>
      </div>
    </div>
  );
}
