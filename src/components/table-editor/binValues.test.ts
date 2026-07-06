// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, it, expect } from "vitest";
import { arrayLength, arrayOf, labelsOf, numericOf } from "./binValues";
import type { ConstantDto, Value } from "../../ipc/bindings";

describe("arrayLength", () => {
  it("returns rows*cols for an Array-kinded constant", () => {
    const c: ConstantDto = {
      name: "wueRates",
      units: "%",
      digits: 0,
      low: 0,
      high: 255,
      kind: { Array: { rows: 3, cols: 1 } },
    };
    expect(arrayLength(c)).toBe(3);
  });

  it("returns null for a non-Array constant or undefined", () => {
    const c: ConstantDto = {
      name: "flag",
      units: "",
      digits: 0,
      low: null,
      high: null,
      kind: "Scalar",
    };
    expect(arrayLength(c)).toBeNull();
    expect(arrayLength(undefined)).toBeNull();
  });
});

describe("arrayOf", () => {
  it("returns the raw Array elements", () => {
    const v: Value = { Array: [1, null, 3] };
    expect(arrayOf(v)).toEqual([1, null, 3]);
  });

  it("returns null for a non-Array value or undefined", () => {
    expect(arrayOf({ Scalar: 5 })).toBeNull();
    expect(arrayOf(undefined)).toBeNull();
  });
});

describe("labelsOf", () => {
  it("formats finite values with the given digits and non-finite as an em dash", () => {
    expect(labelsOf([0, 50.4, null], 0)).toEqual(["0", "50", "—"]);
  });

  it("returns [] for null", () => {
    expect(labelsOf(null, 0)).toEqual([]);
  });
});

describe("numericOf", () => {
  it("maps the null sentinel to NaN", () => {
    const [a, b, c] = numericOf([1, null, 3]);
    expect(a).toBe(1);
    expect(Number.isNaN(b)).toBe(true);
    expect(c).toBe(3);
  });

  it("returns [] for null", () => {
    expect(numericOf(null)).toEqual([]);
  });
});
