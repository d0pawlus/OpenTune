// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from "vitest";
import type { DefinitionDto } from "../../ipc/bindings";
import { resolveMenuTarget } from "./menuTarget";

const definition = {
  dialogs: [{ name: "fuelDialog", title: "", fields: [] }],
  tables: [
    {
      name: "veTableTbl",
      map3d_id: "veTableMap",
      title: "VE Table",
      page: 0,
      x_bins: "",
      x_channel: "",
      y_bins: "",
      y_channel: "",
      z: "",
      xy_labels: [],
      up_down_label: [],
      help: "",
    },
  ],
  curves: [
    {
      name: "dwellCurve",
      title: "",
      column_labels: [],
      x_axis: null,
      y_axis: null,
      x_bins: "",
      x_channel: "",
      y_bins: "",
      gauge: "",
    },
  ],
} as unknown as DefinitionDto;

describe("resolveMenuTarget", () => {
  it("resolves a dialog name to a dialog target", () => {
    expect(resolveMenuTarget(definition, "fuelDialog")).toEqual({
      kind: "dialog",
      name: "fuelDialog",
    });
  });

  it("resolves a table editor name to a table target", () => {
    expect(resolveMenuTarget(definition, "veTableTbl")).toEqual({
      kind: "table",
      name: "veTableTbl",
    });
  });

  it("resolves a table's 3-D map id to the table (rusEFI subMenu = veTableMap)", () => {
    expect(resolveMenuTarget(definition, "veTableMap")).toEqual({
      kind: "table",
      name: "veTableTbl",
    });
  });

  it("resolves a curve name to a curve target", () => {
    expect(resolveMenuTarget(definition, "dwellCurve")).toEqual({
      kind: "curve",
      name: "dwellCurve",
    });
  });

  it("falls back to a dialog target for an unknown name (renders the no-dialog notice)", () => {
    expect(resolveMenuTarget(definition, "mystery")).toEqual({
      kind: "dialog",
      name: "mystery",
    });
  });
});
