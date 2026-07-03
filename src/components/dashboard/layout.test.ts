// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, it, expect } from "vitest";
import {
  defaultSlots,
  moveSlot,
  parseLayout,
  serializeLayout,
  type SlotLayout,
} from "./layout";

const NAMES = ["rpmGauge", "cltGauge", "mapGauge"] as const;

const slots: SlotLayout[] = [
  { gauge: "rpmGauge", kind: "round" },
  { gauge: "cltGauge", kind: "bar" },
];

describe("serializeLayout / parseLayout round trip", () => {
  it("round trips slots through JSON", () => {
    expect(parseLayout(serializeLayout(slots), NAMES)).toEqual(slots);
  });
});

describe("parseLayout (never trusts file content)", () => {
  it("returns null for malformed JSON", () => {
    expect(parseLayout("not json {", NAMES)).toBeNull();
  });

  it("returns null for JSON without a slots array", () => {
    expect(parseLayout("{}", NAMES)).toBeNull();
    expect(parseLayout('{"slots": 3}', NAMES)).toBeNull();
    expect(parseLayout("null", NAMES)).toBeNull();
  });

  it("drops slots naming gauges that no longer exist in the definition", () => {
    const json = serializeLayout([
      { gauge: "rpmGauge", kind: "round" },
      { gauge: "goneGauge", kind: "round" },
    ]);
    expect(parseLayout(json, NAMES)).toEqual([
      { gauge: "rpmGauge", kind: "round" },
    ]);
  });

  it("degrades an unknown gauge kind to round", () => {
    const json =
      '{"version":1,"slots":[{"gauge":"rpmGauge","kind":"hologram"}]}';
    expect(parseLayout(json, NAMES)).toEqual([
      { gauge: "rpmGauge", kind: "round" },
    ]);
  });

  it("skips non-object slot entries", () => {
    const json =
      '{"version":1,"slots":[42, {"gauge":"cltGauge","kind":"bar"}]}';
    expect(parseLayout(json, NAMES)).toEqual([
      { gauge: "cltGauge", kind: "bar" },
    ]);
  });
});

describe("defaultSlots", () => {
  it("maps FrontPage gauge slots to round gauges in slot order", () => {
    expect(
      defaultSlots({
        gauge_slots: ["rpmGauge", "cltGauge"],
        indicators: [],
      }),
    ).toEqual([
      { gauge: "rpmGauge", kind: "round" },
      { gauge: "cltGauge", kind: "round" },
    ]);
  });
});

describe("moveSlot", () => {
  it("swaps a slot with its neighbour and returns a new array", () => {
    const moved = moveSlot(slots, 0, 1);
    expect(moved).toEqual([slots[1], slots[0]]);
    expect(moved).not.toBe(slots);
    // The input is never mutated.
    expect(slots[0]).toEqual({ gauge: "rpmGauge", kind: "round" });
  });

  it("is a no-op at the edges", () => {
    expect(moveSlot(slots, 0, -1)).toEqual(slots);
    expect(moveSlot(slots, 1, 1)).toEqual(slots);
  });
});
