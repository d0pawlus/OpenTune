// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, it, expect } from "vitest";
import { toTsv, parseTsv, pasteEdits } from "./tsv";

describe("toTsv / parseTsv round trip", () => {
  it("round-trips a rect through TSV", () => {
    const g = { rows: 2, cols: 3, values: [1, 2, 3, 4, 5, 6] };
    const tsv = toTsv(g, { r0: 0, c0: 1, r1: 1, c1: 2 }, 0);
    expect(tsv).toBe("2\t3\n5\t6");
    expect(parseTsv(tsv)).toEqual([
      [2, 3],
      [5, 6],
    ]);
  });

  it("formats with digits and accepts comma decimals (PL-locale paste)", () => {
    expect(
      toTsv(
        { rows: 1, cols: 1, values: [12.345] },
        { r0: 0, c0: 0, r1: 0, c1: 0 },
        1,
      ),
    ).toBe("12.3");
    expect(parseTsv("1,5\t2")).toEqual([[1.5, 2]]);
  });

  it("rejects non-numeric cells", () => {
    expect(parseTsv("1\tx")).toBeNull();
  });

  // M4 final-review fix wave item 8 (controller-sanctioned contract change):
  // a blank/whitespace cell now parses to NaN, not 0 — the OLD contract
  // (parseTsv("1\t") -> [[1, 0]]) let a copied NaN-hole silently write 0 onto
  // a finite cell when pasted at an offset. Genuinely non-numeric garbage
  // still rejects the whole block (unchanged).
  it("parses a blank cell as NaN, not 0 (contract change from Task 4)", () => {
    expect(parseTsv("1\t\t3")).toEqual([[1, NaN, 3]]);
  });

  it("drops a single trailing blank line from a pasted block", () => {
    expect(parseTsv("1\t2\n3\t4\n")).toEqual([
      [1, 2],
      [3, 4],
    ]);
  });

  it("renders non-finite cells as blank", () => {
    expect(
      toTsv(
        { rows: 1, cols: 2, values: [1, NaN] },
        { r0: 0, c0: 0, r1: 0, c1: 1 },
        0,
      ),
    ).toBe("1\t");
  });
});

describe("pasteEdits", () => {
  it("clips paste at the grid edge", () => {
    const g = { rows: 2, cols: 2, values: [0, 0, 0, 0] };
    expect(
      pasteEdits(g, { row: 1, col: 1 }, [
        [7, 8],
        [9, 10],
      ]),
    ).toEqual([{ index: 3, value: 7 }]);
  });

  it("pastes a block fully inside the grid", () => {
    const g = { rows: 3, cols: 3, values: [0, 0, 0, 0, 0, 0, 0, 0, 0] };
    expect(
      pasteEdits(g, { row: 0, col: 0 }, [
        [1, 2],
        [3, 4],
      ]),
    ).toEqual([
      { index: 0, value: 1 },
      { index: 1, value: 2 },
      { index: 3, value: 3 },
      { index: 4, value: 4 },
    ]);
  });

  // M4 final-review fix wave item 8: a non-finite SOURCE value (a NaN-hole
  // in the pasted block, post the blank->NaN parseTsv fix above) must be
  // skipped, never turned into a written 0.
  it("skips non-finite SOURCE values instead of writing them", () => {
    const g = { rows: 2, cols: 2, values: [10, 20, 30, 40] };
    expect(
      pasteEdits(g, { row: 0, col: 0 }, [
        [1, NaN],
        [3, 4],
      ]),
    ).toEqual([
      { index: 0, value: 1 },
      { index: 2, value: 3 },
      { index: 3, value: 4 },
    ]);
  });

  it("round-trips a blank cell through toTsv -> parseTsv -> pasteEdits as a skipped edit", () => {
    const g = { rows: 1, cols: 2, values: [1, NaN] };
    const tsv = toTsv(g, { r0: 0, c0: 0, r1: 0, c1: 1 }, 0);
    expect(tsv).toBe("1\t");
    const parsed = parseTsv(tsv);
    expect(parsed).toEqual([[1, NaN]]);
    expect(pasteEdits(g, { row: 0, col: 0 }, parsed!)).toEqual([
      { index: 0, value: 1 },
    ]);
  });
});
