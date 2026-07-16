// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { act, render, screen, fireEvent } from "@testing-library/react";
import { Dashboard } from "./components/dashboard/Dashboard";
import { TunePanel } from "./components/dialogs/TunePanel";
import { DatalogPanel } from "./components/datalog/DatalogPanel";
import * as ipc from "./ipc/bindings";
import type { ConnectionStateEvent, DefinitionDto } from "./ipc/bindings";
import { useConnectionStore } from "./stores/connection";
import { useRealtimeStore } from "./stores/realtime";
import { useDatalogStore } from "./stores/datalog";
import { useTuneStore } from "./stores/tune";

vi.mock("./ipc/bindings", () => ({
  commands: {
    // TunePanel definition/tune lifecycle.
    getDefinition: vi.fn(),
    loadTune: vi.fn(),
    getValues: vi.fn(),
    resolveGaugeBounds: vi.fn(),
    evalConditions: vi.fn(),
    // Dashboard layout + realtime controls.
    loadLayout: vi.fn(),
    saveLayout: vi.fn(),
    startRealtime: vi.fn(),
    stopRealtime: vi.fn(),
    // Offline datalog panel.
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
  events: {
    tuneDirtyEvent: { listen: vi.fn(() => Promise.resolve(() => {})) },
  },
}));

const definition: DefinitionDto = {
  signature: "sig",
  menus: [
    { label: "Fuel", items: [{ label: "Fuel Settings", dialog: "fuel" }] },
  ],
  dialogs: [
    {
      name: "fuel",
      title: "Fuel Settings",
      fields: [{ kind: { Label: "Base fuel" }, visible: null, enable: null }],
    },
  ],
  constants: [],
  tables: [],
  curves: [],
  gauges: [
    {
      name: "rpmGauge",
      channel: "rpm",
      title: "Engine Speed",
      units: "RPM",
      low: 0,
      high: 8000,
      lo_danger: null,
      lo_warn: null,
      hi_warn: null,
      hi_danger: null,
      value_digits: 0,
      label_digits: 0,
      category: "",
    },
  ],
  frontpage: { gauge_slots: ["rpmGauge"], indicators: [] },
  analyze_tables: [],
};

/**
 * Mirrors how `App.tsx` composes the two panels: unconditional siblings over
 * the same `useConnectionStore`/`useTuneStore`/`useRealtimeStore` instances.
 * This is the seam the unit suites cannot see — `TunePanel` owns the
 * definition lifecycle (load + reset) that `Dashboard` mounts from.
 */
function AppPanels() {
  return (
    <>
      <Dashboard locale="en" theme="default" />
      <TunePanel locale="en" />
      <DatalogPanel locale="en" />
    </>
  );
}

/** Mount both panels and wait until each has fully appeared. */
async function renderConnected() {
  const view = render(<AppPanels />);
  await screen.findByRole("img", { name: "Engine Speed" });
  await screen.findByRole("heading", { name: "Tune" });
  return view;
}

const setConnectionState = (state: ConnectionStateEvent) => {
  act(() => {
    useConnectionStore.setState({ connectionState: state });
  });
};

describe("App composition: Dashboard + TunePanel over the shared tune store", () => {
  beforeEach(() => {
    // jsdom has no 2D context — gauges fail open to inert canvases.
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
    useConnectionStore.setState({
      connectionState: { type: "connected", signature: "sig", version: "1" },
    });
    useTuneStore.getState().reset();
    useRealtimeStore.getState().clear();
    useDatalogStore.getState().reset();
    vi.mocked(ipc.commands.getDefinition).mockResolvedValue({
      status: "ok",
      data: definition,
    });
    vi.mocked(ipc.commands.loadTune).mockResolvedValue({
      status: "ok",
      data: null,
    });
    vi.mocked(ipc.commands.getValues).mockResolvedValue({
      status: "ok",
      data: [],
    });
    vi.mocked(ipc.commands.resolveGaugeBounds).mockResolvedValue({
      status: "ok",
      data: [],
    });
    vi.mocked(ipc.commands.evalConditions).mockResolvedValue({
      status: "ok",
      data: [],
    });
    vi.mocked(ipc.commands.loadLayout).mockResolvedValue({
      status: "ok",
      data: null,
    });
    vi.mocked(ipc.commands.startRealtime).mockResolvedValue({
      status: "ok",
      data: null,
    });
    vi.mocked(ipc.commands.stopRealtime).mockResolvedValue({
      status: "ok",
      data: null,
    });
    vi.mocked(ipc.commands.logStatus).mockResolvedValue({
      status: "ok",
      data: { active: false, path: null, format: null, record_count: 0 },
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.clearAllMocks();
    useConnectionStore.setState({ connectionState: null });
    useTuneStore.getState().reset();
    useRealtimeStore.getState().clear();
    useDatalogStore.getState().reset();
  });

  it("keeps both panels mounted and the tune store intact across a reconnect glitch", async () => {
    await renderConnected();

    fireEvent.click(screen.getByRole("button", { name: "Start live" }));
    await screen.findByRole("button", { name: "Stop live" });
    act(() => {
      useRealtimeStore.getState().applyFrame({ channels: [["rpm", 3000]] });
    });

    const definitionBefore = useTuneStore.getState().definition;
    expect(definitionBefore).not.toBeNull();

    // Link glitch: the backend keeps polling armed, so nothing may unmount.
    setConnectionState({ type: "reconnecting", attempt: 1 });

    // The dashboard panel is still mounted with live state intact...
    expect(screen.getByRole("button", { name: "Stop live" })).toBeTruthy();
    expect(screen.getByRole("img", { name: "Engine Speed" })).toBeTruthy();
    // ...TunePanel does not blank...
    expect(screen.getByRole("heading", { name: "Tune" })).toBeTruthy();
    // ...but its wire-touching actions are disabled mid-glitch.
    expect(
      (screen.getByRole("button", { name: "Undo" }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
    // The definition was NOT reset mid-glitch (same object, no refetch), and
    // the last received realtime values are still there for the gauges.
    expect(useTuneStore.getState().definition).toBe(definitionBefore);
    expect(useRealtimeStore.getState().getChannel("rpm")).toBe(3000);

    // Recovery: same panel instances, no reload, actions re-enabled.
    setConnectionState({ type: "connected", signature: "sig", version: "1" });

    expect(screen.getByRole("button", { name: "Stop live" })).toBeTruthy();
    expect(screen.getByRole("heading", { name: "Tune" })).toBeTruthy();
    expect(
      (screen.getByRole("button", { name: "Undo" }) as HTMLButtonElement)
        .disabled,
    ).toBe(false);
    expect(useTuneStore.getState().definition).toBe(definitionBefore);
    // The whole glitch cost zero extra wire traffic from these panels.
    expect(ipc.commands.getDefinition).toHaveBeenCalledTimes(1);
    expect(ipc.commands.loadTune).toHaveBeenCalledTimes(1);
    expect(ipc.commands.startRealtime).toHaveBeenCalledTimes(1);
  });

  it("resets the tune store and unmounts both panels on a true disconnect", async () => {
    const { container } = await renderConnected();
    act(() => {
      useRealtimeStore.getState().applyFrame({ channels: [["rpm", 3000]] });
    });

    setConnectionState({ type: "disconnected" });

    expect(container.firstChild).not.toBeNull();
    expect(screen.queryByRole("heading", { name: "Tune" })).toBeNull();
    expect(
      screen.getByRole("heading", { name: "Datalogs & analysis" }),
    ).toBeTruthy();
    expect(useTuneStore.getState().definition).toBeNull();
    // The dashboard cleared the realtime store on unmount — stale channels
    // must not repaint on the next connect.
    expect(useRealtimeStore.getState().getChannel("rpm")).toBeUndefined();
  });

  it("resets the tune store and unmounts both panels when the link fails for good", async () => {
    const { container } = await renderConnected();

    setConnectionState({ type: "failed", reason: "retries exhausted" });

    expect(container.firstChild).not.toBeNull();
    expect(screen.queryByRole("heading", { name: "Tune" })).toBeNull();
    expect(
      screen.getByRole("heading", { name: "Datalogs & analysis" }),
    ).toBeTruthy();
    expect(useTuneStore.getState().definition).toBeNull();
  });

  // Regression guard: the assembled app must surface a Tables nav when the
  // definition carries tables. Every other fixture uses `tables: []`, so this
  // is the only end-to-end exercise of the definition → TunePanel tables-nav
  // wiring — the exact path a stale dev-server render made look broken during
  // the M4 smoke test.
  it("renders a Tables nav button per definition table over the live link", async () => {
    const table = (
      name: string,
      title: string,
    ): DefinitionDto["tables"][number] => ({
      name,
      map3d_id: "",
      title,
      page: 2,
      x_bins: `${name}_x`,
      x_channel: "rpm",
      y_bins: `${name}_y`,
      y_channel: "fuelLoad",
      z: `${name}_z`,
      xy_labels: [],
      up_down_label: [],
      help: "",
    });
    vi.mocked(ipc.commands.getDefinition).mockResolvedValue({
      status: "ok",
      data: {
        ...definition,
        tables: [
          table("veTable1Tbl", "VE Table"),
          table("afrTable1Tbl", "AFR Target Table"),
        ],
        analyze_tables: ["veTable1Tbl"],
      },
    });

    await renderConnected();

    await screen.findByRole("button", { name: "VE Table" });
    expect(
      screen.getByRole("button", { name: "AFR Target Table" }),
    ).toBeTruthy();
  });

  // rusEFI menus point items straight at table editors (`subMenu =
  // veTableTbl` / its 3-D map id). The clicked item must read as the
  // current one even though the target is a table, not a dialog.
  it("marks a table-target menu item as current after clicking it", async () => {
    const table: DefinitionDto["tables"][number] = {
      name: "veTable1Tbl",
      map3d_id: "veTable1Map",
      title: "VE Table",
      page: 2,
      x_bins: "ve_x",
      x_channel: "rpm",
      y_bins: "ve_y",
      y_channel: "fuelLoad",
      z: "ve_z",
      xy_labels: [],
      up_down_label: [],
      help: "",
    };
    vi.mocked(ipc.commands.getDefinition).mockResolvedValue({
      status: "ok",
      data: {
        ...definition,
        menus: [
          {
            label: "Fuel",
            items: [{ label: "VE (menu)", dialog: "veTable1Map" }],
          },
        ],
        tables: [table],
      },
    });

    await renderConnected();

    const item = await screen.findByRole("button", { name: "VE (menu)" });
    fireEvent.click(item);
    await vi.waitFor(() =>
      expect(item.getAttribute("aria-current")).toBe("true"),
    );
    expect(useTuneStore.getState().activeTable).toBe("veTable1Tbl");
  });
});

/**
 * A file-backed offline tune (loaded via `OfflinePanel`'s `setOfflineDefinition`,
 * simulated here directly on the store) never had a wire link to begin with.
 * `TunePanel`'s reset effect keys off `offline`, not just `isLinkAlive`, so a
 * true disconnect must be a no-op for it — no reset, and no doomed
 * `getValues`/`evalConditions` refresh against a link that was never there
 * (the refresh-guard from commit 0ba29ba, `linkAlive || store.offline`).
 */
describe("App composition: offline tune survives a link disconnect", () => {
  const offlineDefinition: DefinitionDto = {
    signature: "sig-offline",
    menus: [
      { label: "Fuel", items: [{ label: "Fuel Settings", dialog: "fuel" }] },
    ],
    dialogs: [
      {
        name: "fuel",
        title: "Fuel Settings",
        fields: [
          { kind: { Constant: "baseFuel" }, visible: "rpm > 0", enable: null },
        ],
      },
    ],
    constants: [
      {
        name: "baseFuel",
        units: "ms",
        digits: 1,
        low: 0,
        high: 25,
        kind: "Scalar",
      },
    ],
    tables: [],
    curves: [],
    gauges: [],
    frontpage: { gauge_slots: [], indicators: [] },
    analyze_tables: [],
  };

  beforeEach(() => {
    useConnectionStore.setState({ connectionState: null });
    useTuneStore.getState().reset();
    vi.mocked(ipc.commands.getValues).mockResolvedValue({
      status: "ok",
      data: [{ Scalar: 12 }],
    });
    vi.mocked(ipc.commands.evalConditions).mockResolvedValue({
      status: "ok",
      data: [true],
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.clearAllMocks();
    useConnectionStore.setState({ connectionState: null });
    useTuneStore.getState().reset();
  });

  it("keeps the definition after a true disconnect and issues no extra wire reads", async () => {
    act(() => {
      useTuneStore.getState().setOfflineDefinition(offlineDefinition);
      useTuneStore.getState().setActiveDialog("fuel");
    });

    render(<TunePanel locale="en" />);
    await screen.findByRole("heading", { name: "Tune" });

    // Mount reads once against the offline (wire-free) tune — this is the
    // one legitimate call `refresh` makes when `store.offline` is true.
    await vi.waitFor(() =>
      expect(ipc.commands.getValues).toHaveBeenCalledTimes(1),
    );
    await vi.waitFor(() =>
      expect(ipc.commands.evalConditions).toHaveBeenCalledTimes(1),
    );

    const definitionBefore = useTuneStore.getState().definition;
    expect(definitionBefore).toBe(offlineDefinition);
    expect(useTuneStore.getState().offline).toBe(true);

    // A true disconnect: this tune never had a live link, so it must
    // survive untouched — not reset — and must not fire a doomed refresh.
    setConnectionState({ type: "disconnected" });

    expect(useTuneStore.getState().definition).toBe(definitionBefore);
    expect(useTuneStore.getState().offline).toBe(true);
    expect(screen.getByRole("heading", { name: "Tune" })).toBeTruthy();

    // No spurious extra reads fired against the (nonexistent) dead link.
    expect(ipc.commands.getValues).toHaveBeenCalledTimes(1);
    expect(ipc.commands.evalConditions).toHaveBeenCalledTimes(1);
  });
});
