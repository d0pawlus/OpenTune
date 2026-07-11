// SPDX-License-Identifier: GPL-3.0-or-later
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as ipc from "../ipc/bindings";
import { useRealtimeStore } from "./realtime";
import { useDatalogStore } from "./datalog";

vi.mock("../ipc/bindings", () => ({
  commands: {
    openLog: vi.fn(),
    getLogData: vi.fn(),
    startLog: vi.fn(),
    stopLog: vi.fn(),
    logStatus: vi.fn(),
    addLogMarker: vi.fn(),
    saveLog: vi.fn(),
  },
}));

describe("datalog store", () => {
  beforeEach(() => {
    useDatalogStore.getState().reset();
    vi.clearAllMocks();
    vi.mocked(ipc.commands.openLog).mockResolvedValue({
      status: "ok",
      data: {
        fields: [{ name: "rpm", units: "RPM" }],
        record_count: 20_001,
        marker_count: 1,
        duration_ms: 800_000,
      },
    });
    vi.mocked(ipc.commands.getLogData).mockImplementation(async (offset) => {
      const count = offset === 0 ? 20_000 : 1;
      return {
        status: "ok" as const,
        data: {
          offset,
          total_records: 20_001,
          t_ms: Array.from({ length: count }, (_, index) => offset + index),
          columns: [
            Array.from({ length: count }, (_, index) => 1000 + offset + index),
          ],
          markers:
            offset === 0 ? [{ record_index: 10, t_ms: 10, text: "pull" }] : [],
        },
      };
    });
  });

  it("loads bounded pages into stable column arrays", async () => {
    await useDatalogStore.getState().openLog("A", "/tmp/a.csv", "Csv");
    const log = useDatalogStore.getState().logs.A;
    expect(ipc.commands.getLogData).toHaveBeenCalledTimes(2);
    expect(log?.columns.rpm).toHaveLength(20_001);
    expect(log?.markers).toHaveLength(1);
    expect(useDatalogStore.getState().activeSlot).toBe("A");
  });

  it("adds a local derived column and replays it into realtime", async () => {
    await useDatalogStore.getState().openLog("A", "/tmp/a.csv", "Csv");
    useDatalogStore.getState().addMathChannel({
      id: "smooth",
      name: "rpm smooth",
      source: "rpm",
      operation: { kind: "movingAverage", window: 3 },
    });
    useDatalogStore.getState().setPlaybackRow(2);
    expect(useDatalogStore.getState().logs.A?.columns["rpm smooth"][2]).toBe(
      1001,
    );
    expect(useRealtimeStore.getState().getChannel("rpm")).toBe(1002);
    expect(useRealtimeStore.getState().getChannel("rpm smooth")).toBe(1001);
    useDatalogStore.getState().stopPlayback();
    expect(useRealtimeStore.getState().channels).toEqual({});
  });
});
