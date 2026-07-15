// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, it, expect, vi, beforeEach } from "vitest";
import {
  render,
  screen,
  fireEvent,
  within,
  waitFor,
} from "@testing-library/react";
import { TableEditor } from "../table-editor/TableEditor";
import { useTuneStore } from "../../stores/tune";
import * as ipc from "../../ipc/bindings";
import type {
  CaptureStatusDto,
  ConstantDto,
  DefinitionDto,
  TableDto,
  VeAnalysisReportDto,
} from "../../ipc/bindings";

// AutoTunePanel mounts at the bottom of TableEditor (gated on
// `definition.analyze_tables`), so it is exercised here through the real
// TableEditor mount rather than in isolation — the "second role=grid"
// assertion below only makes sense with the base table grid also on screen.
vi.mock("../../ipc/bindings", () => ({
  commands: {
    getValues: vi.fn(),
    setCells: vi.fn(),
    startCapture: vi.fn(),
    stopCapture: vi.fn(),
    captureStatus: vi.fn(),
    runVeAnalyze: vi.fn(),
  },
  events: {
    tuneDirtyEvent: { listen: vi.fn(() => Promise.resolve(() => {})) },
  },
}));

// A 2x2 VE table carrying a [VeAnalyze] map (the sample-INI shape: the real
// bundled INI's veTable1Tbl/veTable names, trimmed to a 2x2 grid).
const table: TableDto = {
  name: "veTable1Tbl",
  map3d_id: "",
  title: "VE Table",
  page: 2,
  x_bins: "rpmBins",
  x_channel: "rpm",
  y_bins: "fuelLoadBins",
  y_channel: "fuelLoad",
  z: "veTable",
  xy_labels: [],
  up_down_label: [],
  help: "",
};

const rpmBins: ConstantDto = {
  name: "rpmBins",
  units: "rpm",
  digits: 0,
  low: null,
  high: null,
  kind: { Array: { rows: 1, cols: 2 } },
};
const fuelLoadBins: ConstantDto = {
  name: "fuelLoadBins",
  units: "kPa",
  digits: 0,
  low: null,
  high: null,
  kind: { Array: { rows: 1, cols: 2 } },
};
const veTable: ConstantDto = {
  name: "veTable",
  units: "%",
  digits: 1,
  low: 0,
  high: 120,
  kind: { Array: { rows: 2, cols: 2 } },
};

function definition(): DefinitionDto {
  return {
    signature: "sig",
    menus: [],
    dialogs: [],
    constants: [rpmBins, fuelLoadBins, veTable],
    tables: [table],
    curves: [],
    gauges: [],
    frontpage: { gauge_slots: [], indicators: [] },
    analyze_tables: ["veTable1Tbl"],
  };
}

function mockGetValues() {
  vi.mocked(ipc.commands.getValues).mockResolvedValue({
    status: "ok",
    data: [
      { Array: [1000, 2000] },
      { Array: [20, 40] },
      { Array: [50, 50, 50, 50] },
    ],
  });
}

const report: VeAnalysisReportDto = {
  table: "veTable1Tbl",
  x_len: 2,
  y_len: 2,
  cells: [
    {
      current: 50,
      proposed: 55,
      delta_pct: 10,
      hit_weight: 8,
      sample_count: 6,
      confidence: 0.8,
    },
    {
      current: 50,
      proposed: 58,
      delta_pct: 16,
      hit_weight: 2,
      sample_count: 1,
      confidence: 0.2,
    },
    {
      current: 50,
      proposed: 50,
      delta_pct: 0,
      hit_weight: 0,
      sample_count: 0,
      confidence: 0,
    },
    {
      current: 50,
      proposed: 50,
      delta_pct: 0,
      hit_weight: 0,
      sample_count: 0,
      confidence: 0,
    },
  ],
  filtered: [{ id: "std_DeadLambda", label: "std_DeadLambda", count: 3 }],
  total_samples: 10,
  used_samples: 7,
};

describe("AutoTunePanel", () => {
  beforeEach(() => {
    useTuneStore.getState().reset();
    vi.clearAllMocks();
    mockGetValues();
    vi.mocked(ipc.commands.runVeAnalyze).mockResolvedValue({
      status: "ok",
      data: report,
    });
    vi.mocked(ipc.commands.setCells).mockResolvedValue({
      status: "ok",
      data: null,
    });
    vi.mocked(ipc.commands.startCapture).mockResolvedValue({
      status: "ok",
      data: null,
    });
    vi.mocked(ipc.commands.stopCapture).mockResolvedValue({
      status: "ok",
      data: { capturing: false, sample_count: 0, duration_ms: 0, dropped: 0 },
    });
    vi.mocked(ipc.commands.captureStatus).mockResolvedValue({
      status: "ok",
      data: { capturing: false, sample_count: 0, duration_ms: 0, dropped: 0 },
    });
    useTuneStore.setState({
      definition: definition(),
      activeTable: "veTable1Tbl",
    });
  });

  it("Analyze renders the delta grid and the visible-filtering list", async () => {
    render(<TableEditor locale="en" />);
    await screen.findByRole("grid");

    fireEvent.click(screen.getByRole("button", { name: "Analyze" }));

    await waitFor(() => expect(screen.getAllByRole("grid")).toHaveLength(2));
    const grids = screen.getAllByRole("grid");
    expect(within(grids[1]).getByText("10.0")).toBeTruthy();
    expect(within(grids[1]).getByText("16.0")).toBeTruthy();
    expect(screen.getByText("std_DeadLambda — 3")).toBeTruthy();
  });

  it("Apply excludes cells below the default 0.5 confidence threshold", async () => {
    render(<TableEditor locale="en" />);
    await screen.findByRole("grid");

    fireEvent.click(screen.getByRole("button", { name: "Analyze" }));
    await waitFor(() => expect(screen.getAllByRole("grid")).toHaveLength(2));

    fireEvent.click(screen.getByRole("button", { name: "Apply proposed" }));

    await waitFor(() =>
      expect(ipc.commands.setCells).toHaveBeenCalledWith("veTable", [
        { index: 0, value: 55 },
      ]),
    );
  });

  it("clearing the confidence-threshold input keeps the previous threshold (never silently 0)", async () => {
    // M4 final-review fix wave item 5 (same class as TableEditor's scale
    // factor): `Number("")` is 0, not NaN — a cleared threshold must not
    // silently become 0 (which would let every cell, however unreliable,
    // through Apply). Proven behaviorally: Apply must still exclude the
    // 0.2-confidence cell after the field is cleared.
    render(<TableEditor locale="en" />);
    await screen.findByRole("grid");

    fireEvent.click(screen.getByRole("button", { name: "Analyze" }));
    await waitFor(() => expect(screen.getAllByRole("grid")).toHaveLength(2));

    const thresholdInput = screen.getByLabelText(
      "Min confidence",
    ) as HTMLInputElement;
    expect(thresholdInput.value).toBe("0.5");

    fireEvent.change(thresholdInput, { target: { value: "" } });

    fireEvent.click(screen.getByRole("button", { name: "Apply proposed" }));

    await waitFor(() =>
      expect(ipc.commands.setCells).toHaveBeenCalledWith("veTable", [
        { index: 0, value: 55 },
      ]),
    );
  });

  it("Apply surfaces a setCells rejection via the error line (no unhandled rejection)", async () => {
    // M4 final-review fix wave item 9: apply() used to fire-and-forget
    // `setCells` (`void ...`), so a backend rejection became an unhandled
    // promise rejection with no visible error — the grid's optimistic
    // proposals would silently roll back with zero explanation.
    render(<TableEditor locale="en" />);
    await screen.findByRole("grid");

    fireEvent.click(screen.getByRole("button", { name: "Analyze" }));
    await waitFor(() => expect(screen.getAllByRole("grid")).toHaveLength(2));

    vi.mocked(ipc.commands.setCells).mockResolvedValue({
      status: "error",
      error: "out of range",
    });

    fireEvent.click(screen.getByRole("button", { name: "Apply proposed" }));

    await waitFor(() => expect(screen.getByText("out of range")).toBeTruthy());
  });

  it("capture Start/Stop call their commands and render sample_count", async () => {
    render(<TableEditor locale="en" />);
    await screen.findByRole("grid");

    const capturingStatus: CaptureStatusDto = {
      capturing: true,
      sample_count: 12,
      duration_ms: 480,
      dropped: 0,
    };
    vi.mocked(ipc.commands.captureStatus).mockResolvedValue({
      status: "ok",
      data: capturingStatus,
    });

    fireEvent.click(screen.getByRole("button", { name: "Start capture" }));
    await waitFor(() => expect(ipc.commands.startCapture).toHaveBeenCalled());
    await waitFor(() => expect(screen.getByText(/Samples: 12/)).toBeTruthy());

    const stoppedStatus: CaptureStatusDto = {
      capturing: false,
      sample_count: 12,
      duration_ms: 480,
      dropped: 0,
    };
    vi.mocked(ipc.commands.stopCapture).mockResolvedValue({
      status: "ok",
      data: stoppedStatus,
    });
    fireEvent.click(screen.getByRole("button", { name: "Stop capture" }));
    await waitFor(() => expect(ipc.commands.stopCapture).toHaveBeenCalled());
  });
});
