// SPDX-License-Identifier: GPL-3.0-or-later
import { beforeEach, describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { DialogEngine } from "./DialogEngine";
import { useTuneStore } from "../../stores/tune";
import * as ipc from "../../ipc/bindings";
import type { ConstantDto, DefinitionDto, Value } from "../../ipc/bindings";

// The embedded table/curve editors (panel = <tableName>) pull bins/cells over
// IPC on mount; mock the module so embedding tests can run without Tauri.
vi.mock("../../ipc/bindings", () => ({
  commands: {
    getValues: vi.fn(async () => ({ status: "ok", data: [] })),
    setCells: vi.fn(),
  },
  events: {
    tuneDirtyEvent: { listen: vi.fn(() => Promise.resolve(() => {})) },
  },
}));

const reqFuel: ConstantDto = {
  name: "reqFuel",
  units: "ms",
  digits: 1,
  low: 0,
  high: 6553.5,
  kind: "Scalar",
};
const injLayout: ConstantDto = {
  name: "injLayout",
  units: "",
  digits: 0,
  low: null,
  high: null,
  kind: { Bits: { options: ["Paired", "Sequential"] } },
};
const crankRPM: ConstantDto = {
  name: "crankRPM",
  units: "rpm",
  digits: 0,
  low: 0,
  high: 6000,
  kind: "Scalar",
};

function definition(): DefinitionDto {
  return {
    signature: "sig",
    menus: [
      { label: "Tuning", items: [{ label: "Engine", dialog: "engine" }] },
    ],
    dialogs: [
      {
        name: "engine",
        title: "Engine Constants",
        fields: [
          { kind: { Label: "Injection" }, visible: null, enable: null },
          { kind: { Constant: "reqFuel" }, visible: null, enable: null },
          { kind: { Constant: "injLayout" }, visible: null, enable: null },
          // Gated field: hidden unless injLayout != 0.
          {
            kind: { Constant: "crankRPM" },
            visible: "injLayout != 0",
            enable: null,
          },
          { kind: "Gap", visible: null, enable: null },
          { kind: { Panel: "advanced" }, visible: null, enable: null },
        ],
      },
      {
        name: "advanced",
        title: "Advanced",
        fields: [
          { kind: { Constant: "reqFuel" }, visible: null, enable: null },
        ],
      },
    ],
    constants: [reqFuel, injLayout, crankRPM],
    tables: [],
    curves: [],
    gauges: [],
    frontpage: { gauge_slots: [], indicators: [] },
    analyze_tables: [],
  };
}

const values: Record<string, Value> = {
  reqFuel: { Scalar: 12.5 },
  injLayout: { Enum: 0 },
  crankRPM: { Scalar: 300 },
};

describe("DialogEngine", () => {
  beforeEach(() => {
    useTuneStore.getState().reset();
    vi.clearAllMocks();
  });

  it("embeds the table editor when a panel names a table (rusEFI veTableDialog)", async () => {
    const def = definition();
    def.tables = [
      {
        name: "veTableTbl",
        map3d_id: "",
        title: "VE Table",
        page: 0,
        x_bins: "veRpmBins",
        x_channel: "",
        y_bins: "veLoadBins",
        y_channel: "",
        z: "veTable",
        xy_labels: [],
        up_down_label: [],
        help: "",
      },
    ];
    def.constants = [
      ...def.constants,
      {
        name: "veTable",
        units: "%",
        digits: 1,
        low: 0,
        high: 120,
        kind: { Array: { rows: 2, cols: 2 } },
      },
      {
        name: "veRpmBins",
        units: "rpm",
        digits: 0,
        low: null,
        high: null,
        kind: { Array: { rows: 1, cols: 2 } },
      },
      {
        name: "veLoadBins",
        units: "kPa",
        digits: 0,
        low: null,
        high: null,
        kind: { Array: { rows: 1, cols: 2 } },
      },
    ];
    def.dialogs.push({
      name: "veTableDialog",
      title: "",
      fields: [{ kind: { Panel: "veTableTbl" }, visible: null, enable: null }],
    });
    vi.mocked(ipc.commands.getValues).mockResolvedValue({
      status: "ok",
      data: [
        { Array: [1000, 2000] },
        { Array: [30, 60] },
        { Array: [50, 51, 52, 53] },
      ],
    });
    render(
      <DialogEngine
        definition={def}
        dialogName="veTableDialog"
        values={{}}
        conditions={{}}
        onEdit={() => {}}
      />,
    );
    // The embedded editor renders its own titled section once values land.
    expect(
      await screen.findByRole("region", { name: "VE Table" }),
    ).toBeTruthy();
  });

  it("embeds the curve editor when a panel names a curve", async () => {
    const def = definition();
    def.curves = [
      {
        name: "dwellCurve",
        title: "Dwell Curve",
        column_labels: [],
        x_axis: null,
        y_axis: null,
        x_bins: "dwellBins",
        x_channel: "",
        y_bins: "dwellValues",
        gauge: "",
      },
    ];
    def.constants = [
      ...def.constants,
      {
        name: "dwellBins",
        units: "rpm",
        digits: 0,
        low: null,
        high: null,
        kind: { Array: { rows: 1, cols: 2 } },
      },
      {
        name: "dwellValues",
        units: "ms",
        digits: 1,
        low: 0,
        high: 10,
        kind: { Array: { rows: 1, cols: 2 } },
      },
    ];
    def.dialogs.push({
      name: "dwellDialog",
      title: "",
      fields: [{ kind: { Panel: "dwellCurve" }, visible: null, enable: null }],
    });
    vi.mocked(ipc.commands.getValues).mockResolvedValue({
      status: "ok",
      data: [{ Array: [1000, 2000] }, { Array: [3.5, 4.0] }],
    });
    render(
      <DialogEngine
        definition={def}
        dialogName="dwellDialog"
        values={{}}
        conditions={{}}
        onEdit={() => {}}
      />,
    );
    expect(
      await screen.findByRole("region", { name: "Dwell Curve" }),
    ).toBeTruthy();
  });

  it("renders bound constant fields, labels, and the dialog title", () => {
    render(
      <DialogEngine
        definition={definition()}
        dialogName="engine"
        values={values}
        conditions={{}}
        onEdit={() => {}}
      />,
    );
    expect(screen.getByText("Engine Constants")).toBeTruthy();
    expect(screen.getByText("Injection")).toBeTruthy(); // Label field
    expect(screen.getAllByLabelText("reqFuel").length).toBeGreaterThan(0);
    expect(screen.getByLabelText("injLayout")).toBeTruthy();
  });

  it("hides a field whose visibility expression evaluates false", () => {
    render(
      <DialogEngine
        definition={definition()}
        dialogName="engine"
        values={values}
        conditions={{ "injLayout != 0": false }}
        onEdit={() => {}}
      />,
    );
    expect(screen.queryByLabelText("crankRPM")).toBeNull();
  });

  it("shows the gated field when its condition is true", () => {
    render(
      <DialogEngine
        definition={definition()}
        dialogName="engine"
        values={values}
        conditions={{ "injLayout != 0": true }}
        onEdit={() => {}}
      />,
    );
    expect(screen.getByLabelText("crankRPM")).toBeTruthy();
  });

  it("recurses into a referenced panel", () => {
    render(
      <DialogEngine
        definition={definition()}
        dialogName="engine"
        values={values}
        conditions={{ "injLayout != 0": true }}
        onEdit={() => {}}
      />,
    );
    // The panel references dialog "advanced", which binds reqFuel again → two
    // reqFuel inputs total (dialog + panel).
    expect(screen.getAllByLabelText("reqFuel").length).toBe(2);
  });

  it("disables a field whose enable expression evaluates false", () => {
    const def = definition();
    def.dialogs[0].fields[1].enable = "injLayout != 0";
    render(
      <DialogEngine
        definition={def}
        dialogName="engine"
        values={values}
        conditions={{ "injLayout != 0": false }}
        onEdit={() => {}}
      />,
    );
    // The dialog's own reqFuel (first) is gated disabled; the panel's is not.
    const inputs = screen.getAllByLabelText("reqFuel") as HTMLInputElement[];
    expect(inputs[0].disabled).toBe(true);
  });
});
