// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, it, expect } from "vitest";
import { axisRange, polylinePoints, cursorFraction } from "./curveMath";

describe("axisRange", () => {
  it("prefers literal DTO bounds over data extents", () => {
    expect(axisRange({ min: -40, max: 215 }, [0, 50, 100])).toEqual({
      min: -40,
      max: 215,
    });
  });

  it("falls back to finite-data extents when bounds are null", () => {
    expect(axisRange({ min: null, max: null }, [10, 20, 5])).toEqual({
      min: 5,
      max: 20,
    });
  });

  it("falls back to finite-data extents when the axis itself is absent", () => {
    expect(axisRange(null, [10, 20, 5])).toEqual({ min: 5, max: 20 });
    expect(axisRange(undefined, [3, 1, 2])).toEqual({ min: 1, max: 3 });
  });

  it("ignores non-finite data points when computing the fallback extent", () => {
    expect(axisRange(null, [NaN, 10, 20])).toEqual({ min: 10, max: 20 });
  });

  it("returns {min: 0, max: 1} when both sources are empty/degenerate", () => {
    expect(axisRange(null, [])).toEqual({ min: 0, max: 1 });
    expect(axisRange({ min: null, max: null }, [NaN])).toEqual({
      min: 0,
      max: 1,
    });
  });

  it("does not fall back when only one bound is literal (the other still null)", () => {
    // Brief pins "falls back ... when bounds are null" (plural) — a single
    // missing bound is treated the same as both missing: use data extents.
    expect(axisRange({ min: -40, max: null }, [10, 20, 5])).toEqual({
      min: 5,
      max: 20,
    });
  });
});

describe("polylinePoints", () => {
  it("maps the pinned two-point example (y inverted, padding applied)", () => {
    expect(
      polylinePoints(
        [0, 100],
        [0, 50],
        { min: 0, max: 100 },
        { min: 0, max: 50 },
        200,
        100,
        10,
      ),
    ).toBe("10,90 190,10");
  });

  it("skips a pair when either x or y is non-finite (uniform non-finite policy)", () => {
    expect(
      polylinePoints(
        [0, 50, 100],
        [0, NaN, 50],
        { min: 0, max: 100 },
        { min: 0, max: 50 },
        200,
        100,
        10,
      ),
    ).toBe("10,90 190,10");
  });

  it("survives a degenerate range without producing NaN/Infinity", () => {
    const points = polylinePoints(
      [5, 5],
      [3, 3],
      { min: 5, max: 5 },
      { min: 3, max: 3 },
      200,
      100,
      10,
    );
    expect(points).not.toMatch(/NaN|Infinity/);
  });
});

describe("cursorFraction", () => {
  it("returns the fraction within range", () => {
    expect(cursorFraction(50, { min: 0, max: 100 })).toBe(0.5);
  });

  it("returns null outside the range", () => {
    expect(cursorFraction(150, { min: 0, max: 100 })).toBeNull();
    expect(cursorFraction(-1, { min: 0, max: 100 })).toBeNull();
  });

  it("returns null for a non-finite value", () => {
    expect(cursorFraction(NaN, { min: 0, max: 100 })).toBeNull();
  });

  it("returns null for a degenerate range", () => {
    expect(cursorFraction(5, { min: 5, max: 5 })).toBeNull();
  });
});
