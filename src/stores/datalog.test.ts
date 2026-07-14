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
        log_id: 1,
        fields: [{ name: "rpm", units: "RPM" }],
        record_count: 20_001,
        marker_count: 1,
        duration_ms: 800_000,
      },
    });
    vi.mocked(ipc.commands.getLogData).mockImplementation(
      async (_logId, offset) => {
        const count = offset === 0 ? 20_000 : 1;
        return {
          status: "ok" as const,
          data: {
            offset,
            total_records: 20_001,
            t_ms: Array.from({ length: count }, (_, index) => offset + index),
            columns: [
              Array.from(
                { length: count },
                (_, index) => 1000 + offset + index,
              ),
            ],
            markers:
              offset === 0
                ? [{ record_index: 10, t_ms: 10, text: "pull" }]
                : [],
          },
        };
      },
    );
  });

  it("loads bounded pages into stable column arrays", async () => {
    await useDatalogStore.getState().openLog("A", "/tmp/a.csv", "Csv");
    const log = useDatalogStore.getState().logs.A;
    expect(ipc.commands.getLogData).toHaveBeenCalledTimes(2);
    // Every page call must carry the log_id the open response minted (M5
    // review CRITICAL — C2), not just the offset/limit.
    expect(ipc.commands.getLogData).toHaveBeenNthCalledWith(1, 1, 0, 20_000);
    expect(ipc.commands.getLogData).toHaveBeenNthCalledWith(
      2,
      1,
      20_000,
      20_000,
    );
    expect(log?.logId).toBe(1);
    expect(log?.columns.rpm).toHaveLength(20_001);
    expect(log?.markers).toHaveLength(1);
    expect(useDatalogStore.getState().activeSlot).toBe("A");
  });

  // M5 review CRITICAL (C2): opening a different log while a page loop is
  // still in flight must never splice the new log's rows into the stale
  // dataset — the backend rejects the superseded id, and the loop must
  // abort and surface the store's normal error state with no partial data.
  it("aborts the paging loop and keeps no partial dataset on a stale log_id", async () => {
    vi.mocked(ipc.commands.getLogData).mockImplementation(
      async (_logId, offset) => {
        if (offset > 0) {
          return {
            status: "error" as const,
            error: "log changed since it was opened",
          };
        }
        return {
          status: "ok" as const,
          data: {
            offset: 0,
            total_records: 20_001,
            t_ms: Array.from({ length: 20_000 }, (_, index) => index),
            columns: [Array.from({ length: 20_000 }, (_, index) => index)],
            markers: [],
          },
        };
      },
    );

    await useDatalogStore.getState().openLog("A", "/tmp/a.csv", "Csv");

    expect(useDatalogStore.getState().logs.A).toBeUndefined();
    expect(useDatalogStore.getState().error).toBe(
      "log changed since it was opened",
    );
    expect(useDatalogStore.getState().loading).toBe(false);
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

  // M2/M3 re-review finding 7: a genuine null gap recorded in the log must
  // clear the gauge while scrubbing over it, not freeze the last reading.
  it("scrubbing over a recorded null gap clears the channel", async () => {
    vi.mocked(ipc.commands.openLog).mockResolvedValue({
      status: "ok",
      data: {
        log_id: 1,
        fields: [{ name: "rpm", units: "RPM" }],
        record_count: 3,
        marker_count: 0,
        duration_ms: 120,
      },
    });
    vi.mocked(ipc.commands.getLogData).mockResolvedValue({
      status: "ok",
      data: {
        offset: 0,
        total_records: 3,
        t_ms: [0, 40, 80],
        columns: [[1000, null, 1200]],
        markers: [],
      },
    });
    await useDatalogStore.getState().openLog("A", "/tmp/gap.csv", "Csv");

    useDatalogStore.getState().setPlaybackRow(0);
    expect(useRealtimeStore.getState().getChannel("rpm")).toBe(1000);

    useDatalogStore.getState().setPlaybackRow(1);
    expect(useRealtimeStore.getState().getChannel("rpm")).toBeUndefined();

    useDatalogStore.getState().setPlaybackRow(2);
    expect(useRealtimeStore.getState().getChannel("rpm")).toBe(1200);
  });

  // H2: once a user plays or scrubs, `replaying: true` gates every live frame
  // off the dashboard (App.tsx). Something in the UI must always be able to
  // reset both flags, or the dashboard is frozen forever.
  describe("playback escape hatch (H2)", () => {
    it("stopPlayback resets both playing and replaying", async () => {
      await useDatalogStore.getState().openLog("A", "/tmp/a.csv", "Csv");
      useDatalogStore.getState().setPlaying(true);
      expect(useDatalogStore.getState().playing).toBe(true);
      expect(useDatalogStore.getState().replaying).toBe(true);

      useDatalogStore.getState().stopPlayback();

      expect(useDatalogStore.getState().playing).toBe(false);
      expect(useDatalogStore.getState().replaying).toBe(false);
      expect(useRealtimeStore.getState().channels).toEqual({});
    });

    it("pausing keeps replaying true — only the explicit Stop clears it", async () => {
      await useDatalogStore.getState().openLog("A", "/tmp/a.csv", "Csv");
      useDatalogStore.getState().setPlaying(true);

      useDatalogStore.getState().setPlaying(false);

      expect(useDatalogStore.getState().playing).toBe(false);
      expect(useDatalogStore.getState().replaying).toBe(true);
    });

    it("unloading the actively replayed dataset resets playback before the reopen resolves", async () => {
      await useDatalogStore.getState().openLog("A", "/tmp/a.csv", "Csv");
      useDatalogStore.getState().setPlaybackRow(1);
      expect(useDatalogStore.getState().replaying).toBe(true);

      const reopened = useDatalogStore
        .getState()
        .openLog("A", "/tmp/a2.csv", "Csv");
      // The unload is synchronous with the call, before the new data even
      // starts loading — the stale replay must never survive it.
      expect(useDatalogStore.getState().replaying).toBe(false);
      expect(useDatalogStore.getState().playing).toBe(false);
      expect(useRealtimeStore.getState().channels).toEqual({});

      await reopened;
      expect(useDatalogStore.getState().logs.A?.path).toBe("/tmp/a2.csv");
    });

    it("opening a different, inactive slot does not disturb an active replay", async () => {
      await useDatalogStore.getState().openLog("A", "/tmp/a.csv", "Csv");
      useDatalogStore.getState().setPlaybackRow(1);
      expect(useDatalogStore.getState().replaying).toBe(true);

      // A/B slots exist for side-by-side comparison — loading B must not
      // kill A's replay, since A still takes selector priority.
      await useDatalogStore.getState().openLog("B", "/tmp/b.csv", "Csv");

      expect(useDatalogStore.getState().replaying).toBe(true);
    });
  });
});
