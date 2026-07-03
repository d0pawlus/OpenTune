// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, it, expect, beforeEach, vi } from "vitest";
import { useTuneStore } from "./tune";
import * as ipc from "../ipc/bindings";
import type { TuneDirtyEvent, Value } from "../ipc/bindings";

// Mock the IPC module: the store calls `commands.setValue` for live writes.
vi.mock("../ipc/bindings", () => ({
  commands: {
    setValue: vi.fn(),
  },
}));

describe("tune store", () => {
  beforeEach(() => {
    useTuneStore.getState().reset();
    vi.clearAllMocks();
  });

  it("starts clean with no values", () => {
    const s = useTuneStore.getState();
    expect(s.dirty).toBe(false);
    expect(s.dirtyPages).toEqual([]);
    expect(s.values).toEqual({});
  });

  it("applyDirty flips the dirty indicator from a tune_dirty event", () => {
    const event: TuneDirtyEvent = { dirty: true, dirty_pages: [1, 2] };
    useTuneStore.getState().applyDirty(event);
    expect(useTuneStore.getState().dirty).toBe(true);
    expect(useTuneStore.getState().dirtyPages).toEqual([1, 2]);

    // Burning emits a clean event.
    useTuneStore.getState().applyDirty({ dirty: false, dirty_pages: [] });
    expect(useTuneStore.getState().dirty).toBe(false);
    expect(useTuneStore.getState().dirtyPages).toEqual([]);
  });

  it("setValue applies an optimistic update immediately", async () => {
    vi.mocked(ipc.commands.setValue).mockResolvedValue({
      status: "ok",
      data: null,
    });
    const next: Value = { Scalar: 12.5 };

    const pending = useTuneStore.getState().setValue("reqFuel", next);
    // Optimistic: the value is present before the command resolves.
    expect(useTuneStore.getState().values.reqFuel).toEqual(next);
    await pending;
    expect(useTuneStore.getState().values.reqFuel).toEqual(next);
  });

  it("setValue rolls back to the previous value when the command errors", async () => {
    useTuneStore.setState({ values: { reqFuel: { Scalar: 3.0 } } });
    vi.mocked(ipc.commands.setValue).mockResolvedValue({
      status: "error",
      error: "`reqFuel`: value 9999 is out of range",
    });

    await expect(
      useTuneStore.getState().setValue("reqFuel", { Scalar: 9999 }),
    ).rejects.toThrow("out of range");

    // Rolled back to the prior value.
    expect(useTuneStore.getState().values.reqFuel).toEqual({ Scalar: 3.0 });
  });

  it("setValue rollback removes a value that had no prior entry", async () => {
    vi.mocked(ipc.commands.setValue).mockResolvedValue({
      status: "error",
      error: "boom",
    });

    await expect(
      useTuneStore.getState().setValue("newConst", { Enum: 2 }),
    ).rejects.toThrow("boom");

    expect("newConst" in useTuneStore.getState().values).toBe(false);
  });
});
