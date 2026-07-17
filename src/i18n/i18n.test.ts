// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, it, expect } from "vitest";
import { t } from "./index";
import { en } from "./en";
import { pl } from "./pl";

describe("i18n", () => {
  it("returns the English string by default", () => {
    expect(t("app.title", "en")).toBe("OpenTune");
  });
  it("returns the Polish string for pl locale", () => {
    expect(t("connection.disconnected", "pl")).toBe("Rozłączono");
  });
  it("falls back to the key when missing", () => {
    // @ts-expect-error testing unknown key
    expect(t("does.not.exist", "en")).toBe("does.not.exist");
  });

  it("keeps every English and Polish message non-empty", () => {
    for (const key of Object.keys(en) as (keyof typeof en)[]) {
      expect(en[key].trim(), `empty English message: ${key}`).not.toBe("");
      expect(pl[key].trim(), `empty Polish message: ${key}`).not.toBe("");
    }
  });

  it("resolves all M6 shell message families in both locales", () => {
    for (const key of [
      "app.heartbeat",
      "update.check",
      "update.available",
      "onboarding.title",
      "onboarding.complete",
    ] as const) {
      expect(t(key, "en")).not.toBe(key);
      expect(t(key, "pl")).not.toBe(key);
    }
  });
});
