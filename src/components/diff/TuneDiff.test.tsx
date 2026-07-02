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

describe("buildMergePayload", () => {
  it("returns the names whose selection is true", () => {
    expect(
      buildMergePayload({ reqFuel: true, clt: false, injLayout: true }),
    ).toEqual(["reqFuel", "injLayout"]);
  });

  it("returns an empty array when nothing is picked", () => {
    expect(buildMergePayload({ reqFuel: false })).toEqual([]);
    expect(buildMergePayload({})).toEqual([]);
  });

  it("ignores keys explicitly set to false after being toggled off", () => {
    // Simulates a checkbox toggled on then off again — the key stays in the
    // record (component state never deletes it) but must not be picked.
    expect(buildMergePayload({ reqFuel: false, clt: true })).toEqual(["clt"]);
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

    await waitFor(() => expect(mergeTune).toHaveBeenCalledWith(["reqFuel"]));
    // merge re-compares; diffTune was called for the initial compare and
    // again after the merge.
    expect(diffTune).toHaveBeenCalledTimes(2);
  });
});
