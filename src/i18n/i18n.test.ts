// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, it, expect } from "vitest";
import { t } from "./index";

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
});
