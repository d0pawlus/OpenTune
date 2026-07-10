// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { act, render, screen, fireEvent } from "@testing-library/react";
import { Dashboard } from "./components/dashboard/Dashboard";
import { TunePanel } from "./components/dialogs/TunePanel";
import * as ipc from "./ipc/bindings";
import type { ConnectionStateEvent, DefinitionDto } from "./ipc/bindings";
import { useConnectionStore } from "./stores/connection";
import { useRealtimeStore } from "./stores/realtime";
import { useTuneStore } from "./stores/tune";

vi.mock("./ipc/bindings", () => ({
  commands: {
    // TunePanel definition/tune lifecycle.
    getDefinition: vi.fn(),
    loadTune: vi.fn(),
    getValues: vi.fn(),
    evalConditions: vi.fn(),
    // Dashboard layout + realtime controls.
    loadLayout: vi.fn(),
    saveLayout: vi.fn(),
    startRealtime: vi.fn(),
    stopRealtime: vi.fn(),
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
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.clearAllMocks();
    useConnectionStore.setState({ connectionState: null });
    useTuneStore.getState().reset();
    useRealtimeStore.getState().clear();
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

    expect(container.firstChild).toBeNull();
    expect(useTuneStore.getState().definition).toBeNull();
    // The dashboard cleared the realtime store on unmount — stale channels
    // must not repaint on the next connect.
    expect(useRealtimeStore.getState().getChannel("rpm")).toBeUndefined();
  });

  it("resets the tune store and unmounts both panels when the link fails for good", async () => {
    const { container } = await renderConnected();

    setConnectionState({ type: "failed", reason: "retries exhausted" });

    expect(container.firstChild).toBeNull();
    expect(useTuneStore.getState().definition).toBeNull();
  });
});
