// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from "vitest";
import { playbackTarget, rowAtTime } from "./playback";

describe("playback helpers", () => {
  it("finds the latest row at or before an irregular timestamp", () => {
    expect(rowAtTime([0, 10, 25, 80], 24)).toBe(1);
    expect(rowAtTime([0, 10, 25, 80], 80)).toBe(3);
  });

  it("clamps before the first row and handles empty logs", () => {
    expect(rowAtTime([100, 200], 0)).toBe(0);
    expect(rowAtTime([], 100)).toBe(0);
  });

  it("scales elapsed wall time deterministically", () => {
    expect(playbackTarget(500, 250, 2)).toBe(1000);
    expect(playbackTarget(500, -1, 2)).toBe(500);
  });
});
