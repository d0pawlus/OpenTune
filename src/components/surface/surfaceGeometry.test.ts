// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, it, expect } from "vitest";
import {
  normalize,
  axisFraction,
  axisFractionIn,
  finiteRange,
  heightOf,
  heightOfIn,
  surfacePositions,
  surfaceIndices,
  surfaceColors,
  bilinearHeight,
} from "./surfaceGeometry";

describe("normalize", () => {
  it("maps min..max onto 0..1", () => {
    expect(normalize([1000, 2000, 3000])).toEqual([0, 0.5, 1]);
  });

  it("maps a degenerate range (equal min/max) to all 0.5", () => {
    expect(normalize([5, 5])).toEqual([0.5, 0.5]);
  });
});

describe("axisFraction", () => {
  it("mirrors normalize for a single physical value", () => {
    expect(axisFraction(2000, [1000, 2000, 3000])).toBe(0.5);
    expect(axisFraction(1000, [1000, 2000, 3000])).toBe(0);
    expect(axisFraction(3000, [1000, 2000, 3000])).toBe(1);
  });

  it("falls back to 0.5 on a degenerate range", () => {
    expect(axisFraction(5, [5, 5])).toBe(0.5);
  });
});

describe("finiteRange", () => {
  it("returns the finite min/max of an array", () => {
    expect(finiteRange([3, 1, 2])).toEqual({ min: 1, max: 3 });
  });

  it("ignores non-finite entries", () => {
    expect(finiteRange([NaN, 1, Infinity, 3])).toEqual({ min: 1, max: 3 });
  });

  it("returns null when no entries are finite", () => {
    expect(finiteRange([NaN, Infinity, -Infinity])).toBeNull();
  });
});

describe("axisFractionIn", () => {
  it("is the range-accepting form of axisFraction: same result given the same range", () => {
    const bins = [1000, 2000, 3000];
    const range = finiteRange(bins);
    for (const v of [1000, 1500, 2000, 3000]) {
      expect(axisFractionIn(range, v)).toBe(axisFraction(v, bins));
    }
  });

  it("falls back to 0.5 on a degenerate range", () => {
    expect(axisFractionIn({ min: 5, max: 5 }, 5)).toBe(0.5);
  });

  it("falls back to 0.5 on a null range", () => {
    expect(axisFractionIn(null, 5)).toBe(0.5);
  });
});

describe("heightOf", () => {
  it("scales a value to heightScale at the data's max, 0 at its min", () => {
    const values = [0, 10, 20, 30];
    expect(heightOf(0, values, 0.5)).toBe(0);
    expect(heightOf(30, values, 0.5)).toBe(0.5);
    expect(heightOf(15, values, 0.5)).toBeCloseTo(0.25);
  });

  it("is 0 for a non-finite value or an all-non-finite array", () => {
    expect(heightOf(NaN, [0, 10], 0.5)).toBe(0);
    expect(heightOf(5, [NaN, NaN], 0.5)).toBe(0);
  });
});

describe("heightOfIn", () => {
  it("is the range-accepting form of heightOf: same result given the same range", () => {
    const values = [0, 10, 20, 30];
    const range = finiteRange(values);
    for (const v of [0, 15, 30]) {
      expect(heightOfIn(range, v, 0.5)).toBe(heightOf(v, values, 0.5));
    }
  });

  it("is 0 for a non-finite value even with a valid range", () => {
    expect(heightOfIn({ min: 0, max: 10 }, NaN, 0.5)).toBe(0);
  });

  it("is 0 on a null or degenerate range", () => {
    expect(heightOfIn(null, 5, 0.5)).toBe(0);
    expect(heightOfIn({ min: 5, max: 5 }, 5, 0.5)).toBe(0);
  });
});

describe("surfacePositions", () => {
  it("lays out row-major [x, y(=height), z] vertices from normalized bins", () => {
    const positions = surfacePositions([0, 1], [0, 1], [0, 10, 20, 30], 0.5);
    expect(positions.length).toBe(12);
    // vertex 0 = row 0, col 0: x=0, height(0)=0, z=0
    expect(Array.from(positions.slice(0, 3))).toEqual([0, 0, 0]);
    // vertex 3 = row 1, col 1: x=1, height(30)=full heightScale, z=1
    expect(Array.from(positions.slice(9, 12))).toEqual([1, 0.5, 1]);
  });

  it("gives a non-finite cell height 0 rather than a normalized fraction", () => {
    const positions = surfacePositions([0, 1], [0, 1], [0, NaN, 20, 30], 0.5);
    expect(positions[4]).toBe(0); // vertex 1's y
  });
});

describe("surfaceIndices", () => {
  it("builds two CCW triangles for a single quad", () => {
    expect(surfaceIndices(2, 2)).toEqual(new Uint32Array([0, 2, 1, 1, 2, 3]));
  });
});

describe("surfaceColors", () => {
  it("uses heatRgb per vertex: low is blue-ish, high is red-ish", () => {
    const colors = surfaceColors([0, 100], 0, 100);
    const [r0, , b0] = colors.slice(0, 3);
    const [r1, , b1] = colors.slice(3, 6);
    expect(b0).toBeGreaterThan(r0);
    expect(r1).toBeGreaterThan(b1);
  });

  it("colors a non-finite cell neutral gray", () => {
    const colors = surfaceColors([0, NaN], 0, 100);
    expect(Array.from(colors.slice(3, 6))).toEqual([0.5, 0.5, 0.5]);
  });
});

describe("bilinearHeight", () => {
  it("interpolates the cell value at physical coordinates", () => {
    expect(bilinearHeight([0, 100], [0, 100], [0, 10, 20, 30], 50, 50)).toBe(
      15,
    );
  });

  it("returns null outside the bin extents", () => {
    expect(
      bilinearHeight([0, 100], [0, 100], [0, 10, 20, 30], -1, 50),
    ).toBeNull();
    expect(
      bilinearHeight([0, 100], [0, 100], [0, 10, 20, 30], 50, 101),
    ).toBeNull();
  });

  it("returns null when a bracketing corner is non-finite", () => {
    expect(
      bilinearHeight([0, 100], [0, 100], [0, NaN, 20, 30], 50, 50),
    ).toBeNull();
  });
});
