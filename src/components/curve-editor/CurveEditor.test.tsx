// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { CurveEditor } from "./CurveEditor";
import { useTuneStore } from "../../stores/tune";
import * as ipc from "../../ipc/bindings";
import type { ConstantDto, CurveDto, DefinitionDto } from "../../ipc/bindings";

// Mock the IPC module: getValues (bin/y-array load) + setCells (gesture
// commit) + tuneDirtyEvent (refetch trigger, unused directly here) — mirrors
// TableEditor.test.tsx's mock (5.3 pattern, reused verbatim for the curve).
vi.mock("../../ipc/bindings", () => ({
  commands: {
    getValues: vi.fn(),
    setCells: vi.fn(),
  },
  events: {
    tuneDirtyEvent: { listen: vi.fn(() => Promise.resolve(() => {})) },
  },
}));

// A 3-bin warmup curve: x = coolant temp bins, y = the editable WUE rates.
const curve: CurveDto = {
  name: "warmupCurve",
  title: "Warmup Enrichment",
  column_labels: [],
  x_axis: null,
  y_axis: null,
  x_bins: "wueBins",
  x_channel: "",
  y_bins: "wueRates",
  gauge: "",
};

const wueBins: ConstantDto = {
  name: "wueBins",
  units: "C",
  digits: 0,
  low: null,
  high: null,
  kind: { Array: { rows: 3, cols: 1 } },
};
const wueRates: ConstantDto = {
  name: "wueRates",
  units: "%",
  digits: 0,
  low: 0,
  high: 255,
  kind: { Array: { rows: 3, cols: 1 } },
};

function definition(): DefinitionDto {
  return {
    signature: "sig",
    menus: [],
    dialogs: [],
    constants: [wueBins, wueRates],
    tables: [],
    curves: [curve],
    gauges: [],
    frontpage: { gauge_slots: [], indicators: [] },
  };
}

function mockGetValues(xs = [0, 50, 100], ys = [10, 20, 30]) {
  vi.mocked(ipc.commands.getValues).mockResolvedValue({
    status: "ok",
    data: [{ Array: xs }, { Array: ys }],
  });
}

describe("CurveEditor", () => {
  beforeEach(() => {
    useTuneStore.getState().reset();
    vi.clearAllMocks();
    vi.mocked(ipc.commands.setCells).mockResolvedValue({
      status: "ok",
      data: null,
    });
    mockGetValues();
    useTuneStore.setState({
      definition: definition(),
      activeCurve: "warmupCurve",
    });
  });

  function surfaceOf(container: HTMLElement): HTMLElement {
    const el = container.querySelector(".ce-surface");
    if (!el) throw new Error("ce-surface not found");
    return el as HTMLElement;
  }

  it("renders a 1x3 grid with ys and x-bin headers", async () => {
    render(<CurveEditor locale="en" />);
    await screen.findByRole("grid");

    expect(screen.getByText("0")).toBeTruthy();
    expect(screen.getByText("50")).toBeTruthy();
    expect(screen.getByText("100")).toBeTruthy();
    expect(screen.getByText("10")).toBeTruthy();
    expect(screen.getByText("20")).toBeTruthy();
    expect(screen.getByText("30")).toBeTruthy();
  });

  it("typing 25 then Enter on the middle cell commits via setCells(y_bins, ...)", async () => {
    const { container } = render(<CurveEditor locale="en" />);
    await screen.findByRole("grid");
    const surface = surfaceOf(container);

    fireEvent.keyDown(surface, { key: "ArrowRight" });
    fireEvent.keyDown(surface, { key: "2" });
    const input = await screen.findByDisplayValue("2");
    fireEvent.change(input, { target: { value: "25" } });
    fireEvent.keyDown(surface, { key: "Enter" });

    await waitFor(() =>
      expect(ipc.commands.setCells).toHaveBeenCalledWith("wueRates", [
        { index: 1, value: 25 },
      ]),
    );
  });

  it("renders the polyline preview with a points attribute for every bin", async () => {
    const { container } = render(<CurveEditor locale="en" />);
    await screen.findByRole("grid");

    const polyline = container.querySelector("polyline");
    expect(polyline).toBeTruthy();
    const points = polyline!.getAttribute("points") ?? "";
    expect(points.trim().split(/\s+/)).toHaveLength(3);
  });
});
