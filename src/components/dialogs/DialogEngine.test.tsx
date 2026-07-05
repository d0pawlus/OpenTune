// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { DialogEngine } from "./DialogEngine";
import type { ConstantDto, DefinitionDto, Value } from "../../ipc/bindings";

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
  };
}

const values: Record<string, Value> = {
  reqFuel: { Scalar: 12.5 },
  injLayout: { Enum: 0 },
  crankRPM: { Scalar: 300 },
};

describe("DialogEngine", () => {
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
