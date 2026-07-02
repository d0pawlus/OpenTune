// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { buildMergePayload, TuneDiff } from "./TuneDiff";

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
    // No diff yet, so no merge action and no table.
    expect(screen.queryByText("Merge selected")).toBeNull();
    expect(screen.queryByRole("table")).toBeNull();
  });

  it("renders the Polish title when locale is pl", () => {
    render(<TuneDiff locale="pl" />);
    expect(screen.getByText("Różnice")).toBeTruthy();
  });
});
