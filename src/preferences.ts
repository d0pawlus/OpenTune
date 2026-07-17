// SPDX-License-Identifier: GPL-3.0-or-later
import type { Theme } from "./components/gauges/GaugeCanvas";
import type { Locale } from "./i18n";

const LOCALE_KEY = "opentune.locale";
const THEME_KEY = "opentune.theme";
const ONBOARDING_KEY = "opentune.onboarding.v1";

function read(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function write(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Preferences are optional; an unavailable store must not block tuning.
  }
}

export function initialLocale(): Locale {
  const saved = read(LOCALE_KEY);
  if (saved === "en" || saved === "pl") return saved;
  return navigator.language.toLowerCase().startsWith("pl") ? "pl" : "en";
}

export function saveLocale(locale: Locale): void {
  write(LOCALE_KEY, locale);
}

export function initialTheme(): Theme {
  return read(THEME_KEY) === "high-contrast" ? "high-contrast" : "default";
}

export function saveTheme(theme: Theme): void {
  write(THEME_KEY, theme);
}

export function isOnboardingComplete(): boolean {
  return read(ONBOARDING_KEY) === "complete";
}

export function completeOnboarding(): void {
  write(ONBOARDING_KEY, "complete");
}
