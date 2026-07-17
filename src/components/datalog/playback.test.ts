// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from "vitest";
import { lastValidTime, playbackTarget, rowAtTime } from "./playback";

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

  // H4: the RAF loop captures one wall-clock base (`startedAt`) and resume
  // point (`startLog`) per play, then every frame maps `now` through
  // `playbackTarget` → `rowAtTime`. Composing them here pins that mechanic:
  // rows advance monotonically off a single time base (the old loop reset the
  // base every frame and drifted), and an overrun clamps to the last row.
  it("advances rows monotonically from a single time base (H4)", () => {
    const tMs = [0, 100, 200, 300, 400];
    const startedAt = 1_000;
    const startLog = tMs[1]; // resumed from row 1
    const speed = 2;
    const rows = [1_050, 1_100, 1_200].map((now) =>
      rowAtTime(tMs, playbackTarget(startLog, now - startedAt, speed)),
    );
    // now=1050 → target 100+100=200 → row 2; 1100 → 300 → row 3;
    // 1200 → 500 (past the end) → clamped to the last row 4.
    expect(rows).toEqual([2, 3, 4]);
  });

  // H4: the playback RAF loop must compute the log's final timestamp without
  // copying/reversing the (up to 100k-element) `tMs` array every frame.
  describe("lastValidTime", () => {
    it("returns the last timestamp when the log ends cleanly", () => {
      expect(lastValidTime([0, 10, 25])).toBe(25);
    });

    it("scans backwards past trailing null gaps", () => {
      expect(lastValidTime([0, 10, 25, null, null])).toBe(25);
    });

    it("returns 0 when every entry is null", () => {
      expect(lastValidTime([null, null, null])).toBe(0);
    });

    it("returns 0 for an empty log", () => {
      expect(lastValidTime([])).toBe(0);
    });
  });
});
