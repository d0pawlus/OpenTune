// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, it, expect } from "vitest";
import {
  rectOf,
  clampCell,
  move,
  cellIndices,
  type Selection,
} from "./selection";

describe("rectOf", () => {
  it("normalizes a selection whose anchor is below/right of its focus", () => {
    const sel: Selection = {
      anchor: { row: 3, col: 2 },
      focus: { row: 1, col: 0 },
    };
    expect(rectOf(sel)).toEqual({ r0: 1, c0: 0, r1: 3, c1: 2 });
  });

  it("normalizes a selection whose anchor is above/left of its focus (already normal)", () => {
    const sel: Selection = {
      anchor: { row: 0, col: 0 },
      focus: { row: 2, col: 3 },
    };
    expect(rectOf(sel)).toEqual({ r0: 0, c0: 0, r1: 2, c1: 3 });
  });

  it("handles a single-cell selection", () => {
    const sel: Selection = {
      anchor: { row: 2, col: 2 },
      focus: { row: 2, col: 2 },
    };
    expect(rectOf(sel)).toEqual({ r0: 2, c0: 2, r1: 2, c1: 2 });
  });
});

describe("clampCell", () => {
  it("clamps negative coordinates to zero", () => {
    expect(clampCell({ row: -1, col: -5 }, 4, 4)).toEqual({ row: 0, col: 0 });
  });

  it("clamps coordinates past the grid edge", () => {
    expect(clampCell({ row: 10, col: 10 }, 4, 4)).toEqual({ row: 3, col: 3 });
  });

  it("leaves in-bounds coordinates untouched", () => {
    expect(clampCell({ row: 1, col: 2 }, 4, 4)).toEqual({ row: 1, col: 2 });
  });
});

describe("move", () => {
  it("moves both anchor and focus together when not extending", () => {
    const sel: Selection = {
      anchor: { row: 1, col: 1 },
      focus: { row: 1, col: 1 },
    };
    const next = move(sel, 1, 0, 4, 4, false);
    expect(next).toEqual({
      anchor: { row: 2, col: 1 },
      focus: { row: 2, col: 1 },
    });
  });

  it("clamps movement at the grid edge", () => {
    const sel: Selection = {
      anchor: { row: 0, col: 0 },
      focus: { row: 0, col: 0 },
    };
    const next = move(sel, -1, -1, 4, 4, false);
    expect(next).toEqual({
      anchor: { row: 0, col: 0 },
      focus: { row: 0, col: 0 },
    });
  });

  it("with extend=true, moves only the focus, keeping the anchor fixed", () => {
    const sel: Selection = {
      anchor: { row: 1, col: 1 },
      focus: { row: 1, col: 1 },
    };
    const next = move(sel, 1, 1, 4, 4, true);
    expect(next).toEqual({
      anchor: { row: 1, col: 1 },
      focus: { row: 2, col: 2 },
    });
  });

  it("clamps the extended focus at the grid edge without moving the anchor", () => {
    const sel: Selection = {
      anchor: { row: 1, col: 1 },
      focus: { row: 3, col: 3 },
    };
    const next = move(sel, 5, 5, 4, 4, true);
    expect(next).toEqual({
      anchor: { row: 1, col: 1 },
      focus: { row: 3, col: 3 },
    });
  });
});

describe("cellIndices", () => {
  it("returns row-major flat indices for a multi-cell rect", () => {
    expect(cellIndices({ r0: 1, c0: 1, r1: 2, c1: 2 }, 4)).toEqual([
      5, 6, 9, 10,
    ]);
  });

  it("returns a single index for a single-cell rect", () => {
    expect(cellIndices({ r0: 0, c0: 0, r1: 0, c1: 0 }, 3)).toEqual([0]);
  });
});
