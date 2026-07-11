// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from "vitest";
import {
  derivative,
  evaluateMathChannel,
  gate,
  lowPass,
  movingAverage,
} from "./mathChannels";

describe("math channels", () => {
  it("calculates derivatives using elapsed seconds", () => {
    expect(derivative([0, 2, 8], [0, 1000, 3000])).toEqual([null, 2, 3]);
  });

  it("resets smoothing at missing samples", () => {
    expect(movingAverage([2, 4, null, 10, 14], 2)).toEqual([
      2,
      3,
      null,
      10,
      12,
    ]);
    expect(lowPass([10, 20, null, 8], 0.5)).toEqual([10, 15, null, 8]);
  });

  it("normalizes reversed gate bounds and rejects non-finite values", () => {
    expect(gate([1, 2, Number.NaN, 3, 4], 3, 2)).toEqual([
      null,
      2,
      null,
      3,
      null,
    ]);
  });

  it("is deterministic for 100k records", () => {
    const values = Array.from({ length: 100_000 }, (_, index) =>
      index % 997 === 0 ? null : Math.sin(index / 50),
    );
    const tMs = Array.from({ length: 100_000 }, (_, index) => index * 40);
    const spec = {
      id: "smooth",
      name: "smooth",
      source: "rpm",
      operation: { kind: "movingAverage" as const, window: 31 },
    };
    const first = evaluateMathChannel(spec, values, tMs);
    const second = evaluateMathChannel(spec, values, tMs);
    expect(first).toEqual(second);
    expect(first).toHaveLength(100_000);
  });
});
