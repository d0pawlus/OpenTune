// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, it, expect } from "vitest";
import { useConnectionStore } from "./connection";

describe("connection store", () => {
  it("starts with no sequence and records the latest", () => {
    expect(useConnectionStore.getState().lastSeq).toBeNull();
    useConnectionStore.getState().setSeq(7);
    expect(useConnectionStore.getState().lastSeq).toBe(7);
  });
});
