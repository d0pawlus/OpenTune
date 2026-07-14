// SPDX-License-Identifier: GPL-3.0-or-later
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import * as ipc from "../../ipc/bindings";
import { useDatalogStore } from "../../stores/datalog";
import { DatalogPanel } from "./DatalogPanel";

vi.mock("../../ipc/bindings", () => ({
  commands: {
    logStatus: vi.fn(),
    startLog: vi.fn(),
    stopLog: vi.fn(),
    addLogMarker: vi.fn(),
    openLog: vi.fn(),
    getLogData: vi.fn(),
    saveLog: vi.fn(),
    logStats: vi.fn(),
    detectAnomaly: vi.fn(),
    virtualDyno: vi.fn(),
  },
}));

// jsdom has no `matchMedia` implementation; the lazily-loaded uPlot chart
// (mounted once a log is loaded) queries it for device-pixel-ratio changes.
if (!window.matchMedia) {
  window.matchMedia = ((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: () => {},
    removeListener: () => {},
    addEventListener: () => {},
    removeEventListener: () => {},
    dispatchEvent: () => false,
  })) as unknown as typeof window.matchMedia;
}

describe("DatalogPanel", () => {
  beforeEach(() => {
    useDatalogStore.getState().reset();
    vi.clearAllMocks();
    vi.mocked(ipc.commands.logStatus).mockResolvedValue({
      status: "ok",
      data: { active: false, path: null, format: null, record_count: 0 },
    });
    vi.mocked(ipc.commands.startLog).mockResolvedValue({
      status: "ok",
      data: {
        active: true,
        path: "/tmp/run.csv",
        format: "Csv",
        record_count: 0,
      },
    });
  });

  it("renders the offline A/B viewer and accessible chart controls", async () => {
    render(<DatalogPanel locale="en" />);
    expect(
      screen.getByRole("heading", { name: "Datalogs & analysis" }),
    ).toBeTruthy();
    expect(screen.getByText(/available offline/i)).toBeTruthy();
    expect(screen.getByRole("group", { name: "Log A" })).toBeTruthy();
    expect(screen.getByRole("group", { name: "Log B" })).toBeTruthy();
    expect(
      screen.getByRole("group", { name: "Shared chart controls" }),
    ).toBeTruthy();
    await waitFor(() => expect(ipc.commands.logStatus).toHaveBeenCalledOnce());
  });

  it("starts CSV recording from an explicit text path", async () => {
    render(<DatalogPanel locale="en" />);
    const recording = screen.getByRole("group", { name: "Recording" });
    fireEvent.change(within(recording).getByLabelText("Explicit file path"), {
      target: { value: "/tmp/run.csv" },
    });
    fireEvent.click(
      within(recording).getByRole("button", { name: "Start recording" }),
    );
    await waitFor(() =>
      expect(ipc.commands.startLog).toHaveBeenCalledWith("/tmp/run.csv", "Csv"),
    );
  });

  // M5 review CRITICAL (C2): while a log load is in flight, re-clicking
  // Open must not be possible — that race is exactly what let a second
  // open splice its rows into the first load's still-paging dataset.
  it("disables the Open button for both slots while a log load is in flight", async () => {
    let resolveOpen: (value: {
      status: "ok";
      data: {
        log_id: number;
        fields: never[];
        record_count: number;
        marker_count: number;
        duration_ms: number;
      };
    }) => void = () => {};
    vi.mocked(ipc.commands.openLog).mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveOpen = resolve;
        }),
    );

    render(<DatalogPanel locale="en" />);
    const logA = screen.getByRole("group", { name: "Log A" });
    const logB = screen.getByRole("group", { name: "Log B" });
    fireEvent.change(within(logA).getByPlaceholderText("/path/to/log.csv"), {
      target: { value: "/tmp/a.csv" },
    });
    // Slot B also has a non-empty path, so its later `disabled` can only be
    // explained by the in-flight load, not by its own empty-path guard.
    fireEvent.change(within(logB).getByPlaceholderText("/path/to/log.csv"), {
      target: { value: "/tmp/b.csv" },
    });
    const openA = within(logA).getByRole("button", {
      name: "Open log",
    }) as HTMLButtonElement;
    const openB = within(logB).getByRole("button", {
      name: "Open log",
    }) as HTMLButtonElement;
    expect(openA.disabled).toBe(false);
    expect(openB.disabled).toBe(false);

    fireEvent.click(openA);
    await waitFor(() => expect(openA.disabled).toBe(true));
    expect(openB.disabled).toBe(true);

    resolveOpen({
      status: "ok",
      data: {
        log_id: 1,
        fields: [],
        record_count: 0,
        marker_count: 0,
        duration_ms: 0,
      },
    });
    await waitFor(() => expect(openA.disabled).toBe(false));
  });

  // H2: once a user scrubs a datalog, `replaying: true` gates every live
  // frame off the dashboard. A Stop control and a visible indicator must
  // always be present so that frozen state is never silent.
  describe("playback escape hatch", () => {
    beforeEach(() => {
      vi.mocked(ipc.commands.openLog).mockResolvedValue({
        status: "ok",
        data: {
          log_id: 1,
          fields: [{ name: "rpm", units: "RPM" }],
          record_count: 3,
          marker_count: 0,
          duration_ms: 80,
        },
      });
      vi.mocked(ipc.commands.getLogData).mockResolvedValue({
        status: "ok",
        data: {
          offset: 0,
          total_records: 3,
          t_ms: [0, 40, 80],
          columns: [[1000, 1100, 1200]],
          markers: [],
        },
      });
    });

    const openLogA = async () => {
      render(<DatalogPanel locale="en" />);
      const logA = screen.getByRole("group", { name: "Log A" });
      fireEvent.change(within(logA).getByPlaceholderText("/path/to/log.csv"), {
        target: { value: "/tmp/a.csv" },
      });
      fireEvent.click(within(logA).getByRole("button", { name: "Open log" }));
      await waitFor(() => expect(ipc.commands.getLogData).toHaveBeenCalled());
    };

    it("hides the replay indicator and disables Stop before any playback starts", async () => {
      await openLogA();
      const playback = screen.getByRole("group", { name: "Playback" });

      expect(screen.queryByText("Replay — live gauges paused")).toBeNull();
      const stop = within(playback).getByRole("button", {
        name: "Stop replay",
      }) as HTMLButtonElement;
      expect(stop.disabled).toBe(true);
    });

    it("shows the replay indicator and an enabled Stop button once scrubbing starts, and Stop returns to live", async () => {
      await openLogA();
      const playback = screen.getByRole("group", { name: "Playback" });

      fireEvent.change(within(playback).getByLabelText("Row position"), {
        target: { value: "1" },
      });

      expect(useDatalogStore.getState().replaying).toBe(true);
      expect(screen.getByText("Replay — live gauges paused")).toBeTruthy();
      const stop = within(playback).getByRole("button", {
        name: "Stop replay",
      }) as HTMLButtonElement;
      expect(stop.disabled).toBe(false);

      fireEvent.click(stop);

      expect(useDatalogStore.getState().replaying).toBe(false);
      expect(useDatalogStore.getState().playing).toBe(false);
      expect(screen.queryByText("Replay — live gauges paused")).toBeNull();
      expect(stop.disabled).toBe(true);
    });
  });

  // M5 review HIGH (H3): a math channel must not leak into analysis
  // requests (the backend rejects unknown names with `MissingChannel`), but
  // it must still be selectable in the chart pickers.
  describe("math channels vs. analysis (H3)", () => {
    beforeEach(() => {
      vi.mocked(ipc.commands.openLog).mockResolvedValue({
        status: "ok",
        data: {
          log_id: 1,
          fields: [{ name: "rpm", units: "RPM" }],
          record_count: 3,
          marker_count: 0,
          duration_ms: 80,
        },
      });
      vi.mocked(ipc.commands.getLogData).mockResolvedValue({
        status: "ok",
        data: {
          offset: 0,
          total_records: 3,
          t_ms: [0, 40, 80],
          columns: [[1000, 1100, 1200]],
          markers: [],
        },
      });
    });

    const openLogAWithMathChannel = async () => {
      render(<DatalogPanel locale="en" />);
      const logA = screen.getByRole("group", { name: "Log A" });
      fireEvent.change(within(logA).getByPlaceholderText("/path/to/log.csv"), {
        target: { value: "/tmp/a.csv" },
      });
      fireEvent.click(within(logA).getByRole("button", { name: "Open log" }));
      await waitFor(() => expect(ipc.commands.getLogData).toHaveBeenCalled());

      const mathLibrary = screen.getByRole("group", {
        name: "Math-channel library",
      });
      fireEvent.change(
        within(mathLibrary).getByLabelText("Derived channel name"),
        { target: { value: "rpm smooth" } },
      );
      fireEvent.click(
        within(mathLibrary).getByRole("button", { name: "Create channel" }),
      );
    };

    it("keeps a math channel selectable in the chart time-series picker", async () => {
      await openLogAWithMathChannel();

      const chartControls = screen.getByRole("group", {
        name: "Shared chart controls",
      });
      const timeChannels = within(chartControls).getByLabelText(
        "Time-series channels",
      );
      expect(within(timeChannels).getByText("rpm smooth")).toBeTruthy();
    });

    it("sends only real field names to logStats when a math channel is defined", async () => {
      vi.mocked(ipc.commands.logStats).mockResolvedValue({
        status: "ok",
        data: {
          total_rows: 3,
          accepted_rows: 3,
          stats: [],
          filtered: [],
          decisions: [],
        },
      });
      await openLogAWithMathChannel();

      fireEvent.click(screen.getByRole("button", { name: "Log statistics" }));

      await waitFor(() =>
        expect(ipc.commands.logStats).toHaveBeenCalledWith(1, {
          channels: ["rpm"],
          reject_when: [],
        }),
      );
    });
  });
});
