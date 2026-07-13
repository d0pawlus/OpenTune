// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, it, expect, beforeEach, vi } from "vitest";
import { useTuneStore } from "./tune";
import * as ipc from "../ipc/bindings";
import type { DefinitionDto, TuneDirtyEvent, Value } from "../ipc/bindings";

// Mock the IPC module: the store calls `commands.setValue`/`setCells` for
// live writes.
vi.mock("../ipc/bindings", () => ({
  commands: {
    setValue: vi.fn(),
    setCells: vi.fn(),
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

  it("mergeValues patches without dropping existing keys", () => {
    useTuneStore.setState({ values: { reqFuel: { Scalar: 12.5 } } });

    useTuneStore.getState().mergeValues({ crankRPM: { Scalar: 300 } });

    expect(useTuneStore.getState().values).toEqual({
      reqFuel: { Scalar: 12.5 },
      crankRPM: { Scalar: 300 },
    });
  });

  it("setCells optimistically patches values[name].Array at the edit indices", async () => {
    useTuneStore.setState({
      values: { veTable: { Array: [10, 20, 30, 40] } },
    });
    vi.mocked(ipc.commands.setCells).mockResolvedValue({
      status: "ok",
      data: null,
    });

    const pending = useTuneStore
      .getState()
      .setCells("veTable", [{ index: 1, value: 99 }]);
    // Optimistic: the patched array is present before the command resolves.
    expect(useTuneStore.getState().values.veTable).toEqual({
      Array: [10, 99, 30, 40],
    });
    await pending;

    expect(ipc.commands.setCells).toHaveBeenCalledWith("veTable", [
      { index: 1, value: 99 },
    ]);
    expect(useTuneStore.getState().values.veTable).toEqual({
      Array: [10, 99, 30, 40],
    });
  });

  it("setCells restores the previous array when the command errors, and rethrows", async () => {
    useTuneStore.setState({
      values: { veTable: { Array: [10, 20, 30, 40] } },
    });
    vi.mocked(ipc.commands.setCells).mockResolvedValue({
      status: "error",
      error: "index 1: value 99 is out of range",
    });

    await expect(
      useTuneStore.getState().setCells("veTable", [{ index: 1, value: 99 }]),
    ).rejects.toThrow("out of range");

    // Rolled back to the prior array.
    expect(useTuneStore.getState().values.veTable).toEqual({
      Array: [10, 20, 30, 40],
    });
  });

  it("setActiveTable clears activeDialog and activeCurve", () => {
    useTuneStore.setState({ activeDialog: "engine", activeCurve: "curve1" });

    useTuneStore.getState().setActiveTable("veTable1Tbl");

    expect(useTuneStore.getState().activeTable).toBe("veTable1Tbl");
    expect(useTuneStore.getState().activeDialog).toBeNull();
    expect(useTuneStore.getState().activeCurve).toBeNull();
  });

  it("setActiveDialog clears activeTable and activeCurve", () => {
    useTuneStore.setState({
      activeTable: "veTable1Tbl",
      activeCurve: "curve1",
    });

    useTuneStore.getState().setActiveDialog("engine");

    expect(useTuneStore.getState().activeDialog).toBe("engine");
    expect(useTuneStore.getState().activeTable).toBeNull();
    expect(useTuneStore.getState().activeCurve).toBeNull();
  });

  it("setActiveCurve clears activeDialog and activeTable", () => {
    useTuneStore.setState({
      activeDialog: "engine",
      activeTable: "veTable1Tbl",
    });

    useTuneStore.getState().setActiveCurve("curve1");

    expect(useTuneStore.getState().activeCurve).toBe("curve1");
    expect(useTuneStore.getState().activeDialog).toBeNull();
    expect(useTuneStore.getState().activeTable).toBeNull();
  });
});

const DEF = {
  signature: "x",
  menus: [],
  dialogs: [],
  constants: [],
  tables: [],
  curves: [],
  gauges: [],
  frontpage: { gaugeSlots: [], indicators: [] },
} as unknown as DefinitionDto;

// M2/M3 re-review finding 4: every refresh() re-resolved gauge bounds and
// rebuilt each entry's object identity even when the values were unchanged.
// Gauge draw callbacks key on that identity, so every mounted gauge's rAF
// paint loop tore down and restarted on every edit/undo/burn.
describe("tune store gauge bounds identity", () => {
  const bound = {
    name: "rpmGauge",
    low: 0,
    high: 8000,
    lo_danger: 300,
    lo_warn: 600,
    hi_warn: 6500,
    hi_danger: 7200,
  };

  beforeEach(() => {
    useTuneStore.getState().reset();
  });

  it("setGaugeBounds preserves entry identity when values are unchanged", () => {
    useTuneStore.getState().setGaugeBounds([bound]);
    const first = useTuneStore.getState().gaugeBounds["rpmGauge"];

    // A fresh DTO with identical values — the refresh() case.
    useTuneStore.getState().setGaugeBounds([{ ...bound }]);

    expect(useTuneStore.getState().gaugeBounds["rpmGauge"]).toBe(first);
  });

  it("setGaugeBounds swaps the entry when a value actually changed", () => {
    useTuneStore.getState().setGaugeBounds([bound]);
    const first = useTuneStore.getState().gaugeBounds["rpmGauge"];

    useTuneStore.getState().setGaugeBounds([{ ...bound, high: 9000 }]);

    const next = useTuneStore.getState().gaugeBounds["rpmGauge"];
    expect(next).not.toBe(first);
    expect(next.high).toBe(9000);
  });
});

describe("tune store offline flag", () => {
  beforeEach(() => useTuneStore.getState().reset());

  it("is offline=false initially", () => {
    expect(useTuneStore.getState().offline).toBe(false);
    expect(useTuneStore.getState().definition).toBeNull();
  });

  it("setOfflineDefinition marks the tune offline", () => {
    useTuneStore.getState().setOfflineDefinition(DEF);
    expect(useTuneStore.getState().offline).toBe(true);
    expect(useTuneStore.getState().definition).not.toBeNull();
  });

  it("setDefinition (online) is not offline", () => {
    useTuneStore.getState().setDefinition(DEF);
    expect(useTuneStore.getState().offline).toBe(false);
  });

  it("reset clears offline + definition", () => {
    useTuneStore.getState().setOfflineDefinition(DEF);
    useTuneStore.getState().reset();
    expect(useTuneStore.getState().offline).toBe(false);
    expect(useTuneStore.getState().definition).toBeNull();
  });
});
