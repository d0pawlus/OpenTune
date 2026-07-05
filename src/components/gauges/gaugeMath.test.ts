// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, it, expect } from "vitest";
import {
  GAUGE_START_ANGLE,
  GAUGE_SWEEP,
  formatValue,
  gaugeGeometry,
  zoneColor,
} from "./gaugeMath";

describe("gaugeGeometry", () => {
  it("maps low → fraction 0 at the start angle", () => {
    const g = gaugeGeometry(0, 0, 100);
    expect(g.fraction).toBe(0);
    expect(g.angle).toBeCloseTo(GAUGE_START_ANGLE);
  });

  it("maps high → fraction 1 at the end of the sweep", () => {
    const g = gaugeGeometry(100, 0, 100);
    expect(g.fraction).toBe(1);
    expect(g.angle).toBeCloseTo(GAUGE_START_ANGLE + GAUGE_SWEEP);
  });

  it("maps mid-range linearly", () => {
    const g = gaugeGeometry(50, 0, 100);
    expect(g.fraction).toBeCloseTo(0.5);
    expect(g.angle).toBeCloseTo(GAUGE_START_ANGLE + 0.5 * GAUGE_SWEEP);
  });

  it("clamps values outside [low, high]", () => {
    expect(gaugeGeometry(-10, 0, 100).fraction).toBe(0);
    expect(gaugeGeometry(200, 0, 100).fraction).toBe(1);
  });

  it("handles a non-zero low bound", () => {
    expect(gaugeGeometry(150, 100, 200).fraction).toBeCloseTo(0.5);
  });

  it("degrades a degenerate range (high <= low) to fraction 0", () => {
    expect(gaugeGeometry(5, 10, 10).fraction).toBe(0);
    expect(gaugeGeometry(5, 20, 10).fraction).toBe(0);
  });

  it("degrades a non-finite value to fraction 0 (fail-open)", () => {
    expect(gaugeGeometry(Number.NaN, 0, 100).fraction).toBe(0);
  });
});

describe("zoneColor", () => {
  const zone = (value: number) => zoneColor(value, 300, 600, 6000, 7000);

  it("classifies mid-range as ok", () => {
    expect(zone(3000)).toBe("ok");
  });

  it("classifies a value at hi_warn as warn", () => {
    expect(zone(6000)).toBe("warn");
  });

  it("classifies a value above hi_danger as danger", () => {
    expect(zone(7500)).toBe("danger");
  });

  it("classifies a value at hi_danger as danger (danger wins over warn)", () => {
    expect(zone(7000)).toBe("danger");
  });

  it("classifies a value at lo_warn as warn", () => {
    expect(zone(600)).toBe("warn");
  });

  it("classifies a value at/below lo_danger as danger", () => {
    expect(zone(300)).toBe("danger");
    expect(zone(100)).toBe("danger");
  });

  it("ignores null thresholds (expression bounds project to null)", () => {
    expect(zoneColor(9999, null, null, null, null)).toBe("ok");
    expect(zoneColor(9999, null, null, null, 10000)).toBe("ok");
    expect(zoneColor(9999, null, null, 9000, null)).toBe("warn");
  });
});

describe("formatValue", () => {
  it("formats with the gauge's value digits", () => {
    expect(formatValue(87.456, 1)).toBe("87.5");
    expect(formatValue(3500, 0)).toBe("3500");
  });

  it("renders an em dash for an unknown value (neutral state)", () => {
    expect(formatValue(undefined, 1)).toBe("—");
  });

  it("renders an em dash for a non-finite value (fail-open)", () => {
    expect(formatValue(Number.NaN, 1)).toBe("—");
  });
});
