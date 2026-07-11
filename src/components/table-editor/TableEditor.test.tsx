// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, it, expect, vi, beforeEach } from "vitest";
import {
  render,
  screen,
  fireEvent,
  within,
  waitFor,
} from "@testing-library/react";
import { TableEditor } from "./TableEditor";
import { useTuneStore } from "../../stores/tune";
import * as ipc from "../../ipc/bindings";
import type { ConstantDto, DefinitionDto, TableDto } from "../../ipc/bindings";

// Mock the IPC module: getValues (bin/cell load) + setCells (gesture commit)
// + tuneDirtyEvent (refetch trigger, unused directly by these assertions).
vi.mock("../../ipc/bindings", () => ({
  commands: {
    getValues: vi.fn(),
    setCells: vi.fn(),
  },
  events: {
    tuneDirtyEvent: { listen: vi.fn(() => Promise.resolve(() => {})) },
  },
}));

// A 2-row (load bins) x 3-col (rpm bins) VE table. NOTE: the Task 5 brief's
// prose describes a "2x2 ConstantDto Array shape" fixture, but a 2x2 grid has
// zero interior cells for `interpolateRect` (every cell is a rect corner), so
// assertion (f) ("clicking Interpolate ... dispatches the Task 4 edits")
// could never observe a non-empty edit set — the frozen `applyEdits` skeleton
// (brief 5.5) early-returns on `edits.length === 0`. Bumped to the minimal
// shape (2x3) that gives a real interior column; documented in
// docs/notes/m4-decisions.md.
const table: TableDto = {
  name: "veTable1Tbl",
  title: "VE Table 1",
  page: 1,
  x_bins: "rpmBins",
  x_channel: "",
  y_bins: "loadBins",
  y_channel: "",
  z: "veTable",
  xy_labels: [],
  up_down_label: ["Up", "Down"],
  help: "",
};

const rpmBins: ConstantDto = {
  name: "rpmBins",
  units: "rpm",
  digits: 0,
  low: null,
  high: null,
  kind: { Array: { rows: 1, cols: 3 } },
};
const loadBins: ConstantDto = {
  name: "loadBins",
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
  kind: { Array: { rows: 2, cols: 3 } },
};

function definition(): DefinitionDto {
  return {
    signature: "sig",
    menus: [],
    dialogs: [],
    constants: [rpmBins, loadBins, veTable],
    tables: [table],
    curves: [],
    gauges: [],
    frontpage: { gauge_slots: [], indicators: [] },
    analyze_tables: [],
  };
}

// Row-major data order: row0 = load 20 -> [60, 61, 62]; row1 = load 80 ->
// [70, 75, 80]. Display reverses rows (top = highest load), so the grid's
// first rendered data row is row1.
const baseZ = [60, 61, 62, 70, 75, 80];

function mockGetValues(z: number[] = baseZ) {
  vi.mocked(ipc.commands.getValues).mockResolvedValue({
    status: "ok",
    data: [{ Array: [2000, 4000, 6000] }, { Array: [20, 80] }, { Array: z }],
  });
}

const writeText = vi.fn().mockResolvedValue(undefined);
const readText = vi.fn().mockResolvedValue("60\t61");

describe("TableEditor", () => {
  beforeEach(() => {
    useTuneStore.getState().reset();
    vi.clearAllMocks();
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText, readText },
      configurable: true,
    });
    writeText.mockClear().mockResolvedValue(undefined);
    readText.mockClear().mockResolvedValue("60\t61");
    vi.mocked(ipc.commands.setCells).mockResolvedValue({
      status: "ok",
      data: null,
    });
    mockGetValues();
    useTuneStore.setState({
      definition: definition(),
      activeTable: "veTable1Tbl",
    });
  });

  function surfaceOf(container: HTMLElement): HTMLElement {
    const el = container.querySelector(".te-surface");
    if (!el) throw new Error("te-surface not found");
    return el as HTMLElement;
  }

  it("(a) renders role=grid with axis headers and display-reversed z rows", async () => {
    render(<TableEditor locale="en" />);
    await screen.findByRole("grid");

    expect(screen.getByText("2000")).toBeTruthy();
    expect(screen.getByText("4000")).toBeTruthy();
    expect(screen.getByText("6000")).toBeTruthy();

    const rows = screen.getAllByRole("row");
    // rows[0] is the xLabels header row; data rows are display-reversed, so
    // the first data row (rows[1]) is load=80 (data row 1), not load=20.
    expect(within(rows[1]).getByText("80")).toBeTruthy();
    expect(within(rows[1]).getByText("70.0")).toBeTruthy();
    expect(within(rows[1]).getByText("75.0")).toBeTruthy();
    expect(within(rows[1]).getByText("80.0")).toBeTruthy();
    expect(within(rows[2]).getByText("20")).toBeTruthy();
    expect(within(rows[2]).getByText("60.0")).toBeTruthy();
    expect(within(rows[2]).getByText("61.0")).toBeTruthy();
    expect(within(rows[2]).getByText("62.0")).toBeTruthy();
  });

  it("(b) ArrowRight moves the active cell (aria-activedescendant changes)", async () => {
    const { container } = render(<TableEditor locale="en" />);
    await screen.findByRole("grid");
    const surface = surfaceOf(container);

    expect(surface.getAttribute("aria-activedescendant")).toBe("veTable1Tbl-0");
    fireEvent.keyDown(surface, { key: "ArrowRight" });
    expect(surface.getAttribute("aria-activedescendant")).toBe("veTable1Tbl-1");
  });

  it("(c) typing 55 then Enter commits the draft via setCells", async () => {
    const { container } = render(<TableEditor locale="en" />);
    await screen.findByRole("grid");
    const surface = surfaceOf(container);

    fireEvent.keyDown(surface, { key: "5" });
    const input = await screen.findByDisplayValue("5");
    fireEvent.change(input, { target: { value: "55" } });
    fireEvent.keyDown(surface, { key: "Enter" });

    await waitFor(() =>
      expect(ipc.commands.setCells).toHaveBeenCalledWith("veTable", [
        { index: 0, value: 55 },
      ]),
    );
  });

  it("(d) Ctrl+C writes TSV of the selection via writeText", async () => {
    const { container } = render(<TableEditor locale="en" />);
    await screen.findByRole("grid");
    const surface = surfaceOf(container);

    fireEvent.keyDown(surface, { key: "c", ctrlKey: true });

    await waitFor(() => expect(writeText).toHaveBeenCalledWith("60.0"));
  });

  it("(e) Ctrl+V calls setCells with the parsed clipboard edits", async () => {
    const { container } = render(<TableEditor locale="en" />);
    await screen.findByRole("grid");
    const surface = surfaceOf(container);

    fireEvent.keyDown(surface, { key: "v", ctrlKey: true });

    await waitFor(() =>
      expect(ipc.commands.setCells).toHaveBeenCalledWith("veTable", [
        { index: 0, value: 60 },
        { index: 1, value: 61 },
      ]),
    );
  });

  it("(i) committing a draft returns keyboard focus to the grid surface", async () => {
    // M4 final-review fix wave item 6: the draft `<input autoFocus>` steals
    // DOM focus from the `.te-surface`; when the draft unmounts on commit,
    // nothing refocuses the surface, so focus falls back to `document.body`
    // and every subsequent keydown (dispatched on whatever the REAL
    // activeElement is, unlike `fireEvent.keyDown(surface, ...)` on a fixed
    // reference) is dead.
    const { container } = render(<TableEditor locale="en" />);
    await screen.findByRole("grid");
    const surface = surfaceOf(container);
    surface.focus();

    fireEvent.keyDown(surface, { key: "5" });
    const input = await screen.findByDisplayValue("5");
    expect(document.activeElement).toBe(input); // autoFocus moved real focus

    fireEvent.change(input, { target: { value: "55" } });
    fireEvent.keyDown(document.activeElement as HTMLElement, { key: "Enter" });

    await waitFor(() => expect(ipc.commands.setCells).toHaveBeenCalled());
    expect(document.activeElement).toBe(surface);

    // Enter's own "commit + move down" already clamped the active cell at
    // the bottom edge (row 0), so exercise the OTHER direction to prove a
    // real move happens post-commit.
    const before = surface.getAttribute("aria-activedescendant");
    fireEvent.keyDown(document.activeElement as HTMLElement, {
      key: "ArrowUp",
    });
    expect(surface.getAttribute("aria-activedescendant")).not.toBe(before);
  });

  it("(g) TSV copy/paste round trip matches the visual (display) row order", async () => {
    // M4 final-review fix wave item 7: the grid renders display-reversed
    // (top = highest data row); copy/paste used to serialize/anchor in raw
    // ascending data-row order, mirroring the block vertically relative to
    // what the user sees. Select the whole 2x3 grid and copy: the TSV's
    // FIRST line must be the visually TOP row (data row 1, load 80).
    const { container } = render(<TableEditor locale="en" />);
    await screen.findByRole("grid");
    const surface = surfaceOf(container);

    fireEvent.keyDown(surface, { key: "a", ctrlKey: true });
    fireEvent.keyDown(surface, { key: "c", ctrlKey: true });

    await waitFor(() => expect(writeText).toHaveBeenCalled());
    const copied = writeText.mock.calls[0][0] as string;
    const lines = copied.split("\n");
    expect(lines[0]).toBe("70.0\t75.0\t80.0");
    expect(lines[1]).toBe("60.0\t61.0\t62.0");

    // Pasting the copied text back at the SAME anchor must be identity.
    readText.mockResolvedValue(copied);
    fireEvent.keyDown(surface, { key: "v", ctrlKey: true });

    await waitFor(() =>
      expect(ipc.commands.setCells).toHaveBeenCalledWith("veTable", [
        { index: 0, value: 60 },
        { index: 1, value: 61 },
        { index: 2, value: 62 },
        { index: 3, value: 70 },
        { index: 4, value: 75 },
        { index: 5, value: 80 },
      ]),
    );
  });

  it("(h) clearing the scale-factor input disables Apply (never sends factor 0)", async () => {
    // M4 final-review fix wave item 5: `Number("")` is `0`, not `NaN`, so the
    // old `Number.isNaN` guard let a cleared scale input zero the whole
    // selection. An empty/unparseable factor must be a no-op — the button
    // itself is disabled.
    render(<TableEditor locale="en" />);
    await screen.findByRole("grid");

    const scaleInput = screen.getByLabelText(
      "Scale factor",
    ) as HTMLInputElement;
    const applyButton = screen.getByRole("button", {
      name: "Apply",
    }) as HTMLButtonElement;
    expect(applyButton.disabled).toBe(false);

    fireEvent.change(scaleInput, { target: { value: "" } });
    expect(applyButton.disabled).toBe(true);

    fireEvent.click(applyButton);
    expect(ipc.commands.setCells).not.toHaveBeenCalled();
  });

  it("(f) clicking Interpolate with a multi-cell selection dispatches the Task 4 edits", async () => {
    const { container } = render(<TableEditor locale="en" />);
    await screen.findByRole("grid");
    const surface = surfaceOf(container);

    // Extend the active cell (row0/col0) across the whole row (3 cols) —
    // the only rect shape in a 2x3 grid with an interior (non-corner) cell.
    fireEvent.keyDown(surface, { key: "ArrowRight", shiftKey: true });
    fireEvent.keyDown(surface, { key: "ArrowRight", shiftKey: true });

    fireEvent.click(screen.getByRole("button", { name: "Interpolate" }));

    await waitFor(() =>
      expect(ipc.commands.setCells).toHaveBeenCalledWith("veTable", [
        { index: 1, value: 61 },
      ]),
    );
  });
});
