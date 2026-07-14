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
