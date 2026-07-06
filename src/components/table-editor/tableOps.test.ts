// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, it, expect } from "vitest";
import {
  interpolateRect,
  smoothRect,
  scaleRect,
  setEqualRect,
  stepRect,
  type Grid,
} from "./tableOps";

const grid = (rows: number, cols: number, values: number[]): Grid => ({
  rows,
  cols,
  values,
});

describe("interpolateRect", () => {
  it("linearly fills a 1xN run between its endpoints", () => {
    const g = grid(1, 4, [10, 0, 0, 40]);
    expect(interpolateRect(g, { r0: 0, c0: 0, r1: 0, c1: 3 })).toEqual([
      { index: 1, value: 20 },
      { index: 2, value: 30 },
    ]);
  });
  it("bilinearly fills a 3x3 rect from its corners", () => {
    const v = [0, 0, 20, 0, 99, 0, 40, 0, 60]; // corners 0,20,40,60 → center 30
    const edits = interpolateRect(grid(3, 3, v), {
      r0: 0,
      c0: 0,
      r1: 2,
      c1: 2,
    });
    expect(edits.find((e) => e.index === 4)?.value).toBe(30);
    expect(edits.some((e) => [0, 2, 6, 8].includes(e.index))).toBe(false); // corners kept
  });
  it("is a no-op on a single cell", () => {
    expect(
      interpolateRect(grid(2, 2, [1, 2, 3, 4]), { r0: 0, c0: 0, r1: 0, c1: 0 }),
    ).toEqual([]);
  });
  it("returns [] when a rect corner is non-finite (can't anchor on NaN)", () => {
    const v = [0, 0, 20, 0, 99, 0, NaN, 0, 60]; // bottom-left corner is NaN
    expect(
      interpolateRect(grid(3, 3, v), { r0: 0, c0: 0, r1: 2, c1: 2 }),
    ).toEqual([]);
  });
});

describe("smoothRect", () => {
  it("pulls a spike toward its neighbors, writing only inside the rect", () => {
    const g = grid(3, 3, [50, 50, 50, 50, 90, 50, 50, 50, 50]);
    const edits = smoothRect(g, { r0: 1, c0: 1, r1: 1, c1: 1 });
    expect(edits).toHaveLength(1);
    expect(edits[0].index).toBe(4);
    expect(edits[0].value).toBeCloseTo((90 * 4 + 50 * 8 + 50 * 4) / 16, 6);
    expect(edits[0].value).toBeLessThan(90);
  });
  it("renormalizes the kernel at the grid corner", () => {
    const g = grid(2, 2, [80, 40, 40, 40]);
    const [e] = smoothRect(g, { r0: 0, c0: 0, r1: 0, c1: 0 });
    expect(e.value).toBeCloseTo((80 * 4 + 40 * 2 + 40 * 2 + 40 * 1) / 9, 6);
  });
});

describe("scaleRect / setEqualRect / stepRect", () => {
  it("scales only the selection", () => {
    expect(
      scaleRect(grid(1, 3, [10, 20, 30]), { r0: 0, c0: 1, r1: 0, c1: 2 }, 1.1),
    ).toEqual([
      { index: 1, value: 22 },
      { index: 2, value: 33 },
    ]);
  });
  it("set-equal defaults to the selection mean, skipping non-finite cells", () => {
    expect(
      setEqualRect(grid(1, 3, [10, NaN, 30]), { r0: 0, c0: 0, r1: 0, c1: 2 }),
    ).toEqual([
      { index: 0, value: 20 },
      { index: 2, value: 20 },
    ]);
  });
  it("set-equal honors an explicit value override", () => {
    expect(
      setEqualRect(grid(1, 2, [1, 2]), { r0: 0, c0: 0, r1: 0, c1: 1 }, 5),
    ).toEqual([
      { index: 0, value: 5 },
      { index: 1, value: 5 },
    ]);
  });
  it("set-equal returns [] when every selected cell is non-finite", () => {
    expect(
      setEqualRect(grid(1, 2, [NaN, NaN]), { r0: 0, c0: 0, r1: 0, c1: 1 }),
    ).toEqual([]);
  });
  it("steps every selected cell by delta", () => {
    expect(
      stepRect(grid(1, 2, [10, 20]), { r0: 0, c0: 0, r1: 0, c1: 1 }, -1),
    ).toEqual([
      { index: 0, value: 9 },
      { index: 1, value: 19 },
    ]);
  });
  it("scale/step never touch non-finite cells", () => {
    expect(
      scaleRect(grid(1, 2, [NaN, 10]), { r0: 0, c0: 0, r1: 0, c1: 1 }, 2),
    ).toEqual([{ index: 1, value: 20 }]);
    expect(
      stepRect(grid(1, 2, [NaN, 10]), { r0: 0, c0: 0, r1: 0, c1: 1 }, 1),
    ).toEqual([{ index: 1, value: 11 }]);
  });
});
