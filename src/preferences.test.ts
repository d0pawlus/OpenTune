// SPDX-License-Identifier: GPL-3.0-or-later
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  completeOnboarding,
  initialLocale,
  initialTheme,
  isOnboardingComplete,
  saveLocale,
  saveTheme,
} from "./preferences";

describe("preferences", () => {
  beforeEach(() => {
    localStorage.clear();
    Object.defineProperty(window.navigator, "language", {
      configurable: true,
      value: "en-US",
    });
  });

  it("uses a saved locale before the browser language", () => {
    localStorage.setItem("opentune.locale", "en");
    Object.defineProperty(window.navigator, "language", {
      configurable: true,
      value: "pl-PL",
    });

    expect(initialLocale()).toBe("en");
  });

  it("uses Polish browser language when no locale is saved", () => {
    Object.defineProperty(window.navigator, "language", {
      configurable: true,
      value: "pl-PL",
    });

    expect(initialLocale()).toBe("pl");
  });

  it("falls back to English for an unsupported browser language", () => {
    Object.defineProperty(window.navigator, "language", {
      configurable: true,
      value: "de-DE",
    });

    expect(initialLocale()).toBe("en");
  });

  it("ignores corrupt locale and theme values", () => {
    localStorage.setItem("opentune.locale", "xx");
    localStorage.setItem("opentune.theme", "neon");

    expect(initialLocale()).toBe("en");
    expect(initialTheme()).toBe("default");
  });

  it("loads and writes supported locale and theme choices", () => {
    saveLocale("pl");
    saveTheme("high-contrast");

    expect(initialLocale()).toBe("pl");
    expect(initialTheme()).toBe("high-contrast");
  });

  it("tracks the versioned onboarding completion flag", () => {
    expect(isOnboardingComplete()).toBe(false);

    completeOnboarding();

    expect(isOnboardingComplete()).toBe(true);
    expect(localStorage.getItem("opentune.onboarding.v1")).toBe("complete");
  });

  it("fails open when storage is unavailable", () => {
    const getItem = vi
      .spyOn(Storage.prototype, "getItem")
      .mockImplementation(() => {
        throw new Error("storage disabled");
      });

    expect(initialLocale()).toBe("en");
    expect(initialTheme()).toBe("default");
    expect(isOnboardingComplete()).toBe(false);

    getItem.mockRestore();
  });
});
