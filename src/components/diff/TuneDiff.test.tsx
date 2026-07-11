// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { buildMergePayload, TuneDiff } from "./TuneDiff";
import * as ipc from "../../ipc/bindings";
import type { FieldDiffDto } from "../../ipc/bindings";

vi.mock("../../ipc/bindings", () => ({
  commands: {
    snapshotTune: vi.fn(),
    diffTune: vi.fn(),
    mergeTune: vi.fn(),
  },
}));

const reqFuelDiff: FieldDiffDto = {
  name: "reqFuel",
  a: { Scalar: 12.5 },
  b: { Scalar: 0 },
  cells: [],
};

const mapDiff: FieldDiffDto = {
  name: "veTable",
  a: { Array: [10, 20, 30] },
  b: { Array: [11, 22, 30] },
  cells: [
    { index: 0, a: 10, b: 11 },
    { index: 1, a: 20, b: 22 },
  ],
};

describe("buildMergePayload", () => {
  it("returns complete fields and selected cell indices", () => {
    expect(
      buildMergePayload({
        reqFuel: { all: true, cells: {} },
        veTable: { all: false, cells: { 0: true, 1: false, 3: true } },
      }),
    ).toEqual([
      { type: "all", name: "reqFuel" },
      { type: "cells", name: "veTable", indices: [0, 3] },
    ]);
  });

  it("returns an empty array when nothing is picked", () => {
    expect(
      buildMergePayload({ reqFuel: { all: false, cells: {} } }),
    ).toEqual([]);
    expect(buildMergePayload({})).toEqual([]);
  });
});

describe("TuneDiff", () => {
  it("renders the snapshot action and prompts for a baseline before any diff exists", () => {
    render(<TuneDiff locale="en" />);
    expect(screen.getByText("Snapshot baseline")).toBeTruthy();
    expect(
      screen.getByText("Snapshot the current tune to start comparing"),
    ).toBeTruthy();
    // No baseline yet, so no compare/merge actions and no table.
    expect(screen.queryByText("Compare")).toBeNull();
    expect(screen.queryByText("Merge selected")).toBeNull();
    expect(screen.queryByRole("table")).toBeNull();
  });

  it("renders the Polish title when locale is pl", () => {
    render(<TuneDiff locale="pl" />);
    expect(screen.getByText("Różnice")).toBeTruthy();
  });

  it("populates the diff table after snapshotting, and merges the picked row", async () => {
    const snapshotTune = vi.mocked(ipc.commands.snapshotTune);
    const diffTune = vi.mocked(ipc.commands.diffTune);
    const mergeTune = vi.mocked(ipc.commands.mergeTune);

    snapshotTune.mockResolvedValue({ status: "ok", data: null });
    diffTune.mockResolvedValue({ status: "ok", data: [reqFuelDiff] });
    mergeTune.mockResolvedValue({ status: "ok", data: null });

    render(<TuneDiff locale="en" />);

    // Snapshotting captures the baseline and immediately compares against it.
    fireEvent.click(screen.getByText("Snapshot baseline"));
    await waitFor(() => expect(screen.getByRole("table")).toBeTruthy());
    expect(screen.getByLabelText("reqFuel")).toBeTruthy();

    // A later, independent "Compare" re-diffs without re-snapshotting — the
    // real usage is snapshot once, edit elsewhere, compare repeatedly.
    expect(screen.getByText("Compare")).toBeTruthy();

    // Pick the row and merge — the checkbox -> payload -> command wiring.
    fireEvent.click(screen.getByLabelText("reqFuel"));
    fireEvent.click(screen.getByText("Merge selected"));

    await waitFor(() =>
      expect(mergeTune).toHaveBeenCalledWith([
        { type: "all", name: "reqFuel" },
      ]),
    );
    // merge re-compares; diffTune was called for the initial compare and
    // again after the merge. The component awaits mergeTune before invoking
    // compare(), so the second diffTune call lands strictly after the wait
    // above resolved — assert it with its own waitFor.
    await waitFor(() => expect(diffTune).toHaveBeenCalledTimes(2));
  });

  it("renders and merges individual changed table cells", async () => {
    vi.mocked(ipc.commands.snapshotTune).mockResolvedValue({
      status: "ok",
      data: null,
    });
    vi.mocked(ipc.commands.diffTune).mockResolvedValue({
      status: "ok",
      data: [mapDiff],
    });
    vi.mocked(ipc.commands.mergeTune).mockResolvedValue({
      status: "ok",
      data: null,
    });

    render(<TuneDiff locale="en" />);
    fireEvent.click(screen.getByText("Snapshot baseline"));
    await waitFor(() => expect(screen.getByLabelText("veTable[1]")).toBeTruthy());
    fireEvent.click(screen.getByLabelText("veTable[1]"));
    fireEvent.click(screen.getByText("Merge selected"));
    await waitFor(() =>
      expect(ipc.commands.mergeTune).toHaveBeenCalledWith([
        { type: "cells", name: "veTable", indices: [1] },
      ]),
    );
  });

  it("preserves a partial merge error while refreshing state and diff", async () => {
    const afterMerge = vi.fn().mockResolvedValue(undefined);
    vi.mocked(ipc.commands.snapshotTune).mockResolvedValue({
      status: "ok",
      data: null,
    });
    vi.mocked(ipc.commands.diffTune).mockResolvedValue({
      status: "ok",
      data: [reqFuelDiff],
    });
    vi.mocked(ipc.commands.mergeTune).mockResolvedValue({
      status: "error",
      error: "write failed after an earlier pick",
    });

    render(<TuneDiff locale="en" onAfterMerge={afterMerge} />);
    fireEvent.click(screen.getByText("Snapshot baseline"));
    await waitFor(() => expect(screen.getByLabelText("reqFuel")).toBeTruthy());
    fireEvent.click(screen.getByLabelText("reqFuel"));
    fireEvent.click(screen.getByText("Merge selected"));

    await waitFor(() =>
      expect(screen.getByText("write failed after an earlier pick")).toBeTruthy(),
    );
    expect(afterMerge).toHaveBeenCalledTimes(1);
    expect(ipc.commands.diffTune).toHaveBeenCalledTimes(2);
  });
});
