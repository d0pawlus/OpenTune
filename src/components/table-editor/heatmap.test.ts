// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, it, expect } from "vitest";
import { heatT, heatColor, heatRgb } from "./heatmap";

describe("heatmap", () => {
  it("maps low→blue and high→red, clamps, and survives a degenerate range", () => {
    expect(heatColor(0, 0, 100)).toBe("hsl(220 70% 55%)");
    expect(heatColor(100, 0, 100)).toBe("hsl(0 70% 55%)");
    expect(heatT(-5, 0, 100)).toBe(0);
    expect(heatT(50, 100, 100)).toBe(0.5); // degenerate lo >= hi
    const [r, g, b] = heatRgb(50, 0, 100);
    for (const c of [r, g, b]) expect(c).toBeGreaterThanOrEqual(0);
    for (const c of [r, g, b]) expect(c).toBeLessThanOrEqual(1);
  });

  it("clamps heatT above the range to 1", () => {
    expect(heatT(500, 0, 100)).toBe(1);
  });

  it("clamps heatColor for out-of-range values instead of extrapolating hue", () => {
    expect(heatColor(-50, 0, 100)).toBe("hsl(220 70% 55%)");
    expect(heatColor(150, 0, 100)).toBe("hsl(0 70% 55%)");
  });
});
