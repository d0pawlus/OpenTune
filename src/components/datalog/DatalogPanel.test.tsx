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
});
