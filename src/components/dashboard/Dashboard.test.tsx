// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import {
  act,
  render,
  screen,
  fireEvent,
  waitFor,
} from "@testing-library/react";
import { Dashboard } from "./Dashboard";
import * as ipc from "../../ipc/bindings";
import type { DefinitionDto, GaugeDto } from "../../ipc/bindings";
import { useConnectionStore } from "../../stores/connection";
import { useTuneStore } from "../../stores/tune";
import { serializeLayout } from "./layout";

vi.mock("../../ipc/bindings", () => ({
  commands: {
    loadLayout: vi.fn(),
    saveLayout: vi.fn(),
    startRealtime: vi.fn(),
    stopRealtime: vi.fn(),
  },
}));

const gauge = (name: string, channel: string, title: string): GaugeDto => ({
  name,
  channel,
  title,
  units: "",
  low: 0,
  high: 100,
  lo_danger: null,
  lo_warn: null,
  hi_warn: null,
  hi_danger: null,
  value_digits: 0,
  label_digits: 0,
  category: "",
});

const definition = (overrides?: Partial<DefinitionDto>): DefinitionDto => ({
  signature: "sig",
  menus: [],
  dialogs: [],
  constants: [],
  tables: [],
  curves: [],
  analyze_tables: [],
  gauges: [
    gauge("rpmGauge", "rpm", "Engine Speed"),
    gauge("cltGauge", "clt", "Coolant"),
  ],
  frontpage: {
    gauge_slots: ["rpmGauge", "cltGauge"],
    indicators: [
      {
        expr: "running",
        off_label: "Not running",
        on_label: "Running",
        off_bg: "black",
        off_fg: "white",
        on_bg: "green",
        on_fg: "black",
      },
    ],
  },
  ...overrides,
});

describe("Dashboard", () => {
  beforeEach(() => {
    // jsdom has no 2D context — gauges fail open to inert canvases.
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
    useConnectionStore.setState({
      connectionState: { type: "connected", signature: "sig", version: "1" },
    });
    useTuneStore.setState({ definition: definition() });
    vi.mocked(ipc.commands.loadLayout).mockResolvedValue({
      status: "ok",
      data: null,
    });
    vi.mocked(ipc.commands.saveLayout).mockResolvedValue({
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
    useConnectionStore.setState({ connectionState: null });
    useTuneStore.setState({ definition: null });
  });

  it("renders nothing when disconnected", () => {
    useConnectionStore.setState({ connectionState: { type: "disconnected" } });
    const { container } = render(<Dashboard locale="en" theme="default" />);
    expect(container.firstChild).toBeNull();
  });

  it("renders the FrontPage slots and indicators when no layout is persisted", async () => {
    render(<Dashboard locale="en" theme="default" />);
    expect(
      await screen.findByRole("img", { name: "Engine Speed" }),
    ).toBeTruthy();
    expect(screen.getByRole("img", { name: "Coolant" })).toBeTruthy();
    expect(screen.getByRole("img", { name: "Running" })).toBeTruthy();
    expect(screen.getByText("Dashboard")).toBeTruthy();
  });

  it("prefers a persisted layout over the FrontPage defaults", async () => {
    vi.mocked(ipc.commands.loadLayout).mockResolvedValue({
      status: "ok",
      data: serializeLayout([{ gauge: "cltGauge", kind: "bar" }]),
    });
    render(<Dashboard locale="en" theme="default" />);
    expect(await screen.findByRole("img", { name: "Coolant" })).toBeTruthy();
    expect(screen.queryByRole("img", { name: "Engine Speed" })).toBeNull();
  });

  it("falls back to the FrontPage when the persisted layout is corrupt", async () => {
    vi.mocked(ipc.commands.loadLayout).mockResolvedValue({
      status: "ok",
      data: "not json {",
    });
    render(<Dashboard locale="en" theme="default" />);
    expect(
      await screen.findByRole("img", { name: "Engine Speed" }),
    ).toBeTruthy();
    expect(screen.getByRole("img", { name: "Coolant" })).toBeTruthy();
  });

  it("renders a neutral tile for a slot whose gauge the INI no longer defines", async () => {
    useTuneStore.setState({
      definition: definition({
        frontpage: {
          gauge_slots: ["rpmGauge", "ghostGauge"],
          indicators: [],
        },
      }),
    });
    render(<Dashboard locale="en" theme="default" />);
    expect(
      await screen.findByRole("img", { name: "Engine Speed" }),
    ).toBeTruthy();
    expect(screen.getByRole("img", { name: "ghostGauge" })).toBeTruthy();
  });

  it("start/stop live toggles the realtime commands", async () => {
    render(<Dashboard locale="en" theme="default" />);
    const toggle = await screen.findByRole("button", { name: "Start live" });
    fireEvent.click(toggle);
    await waitFor(() =>
      expect(ipc.commands.startRealtime).toHaveBeenCalledTimes(1),
    );
    const stop = await screen.findByRole("button", { name: "Stop live" });
    fireEvent.click(stop);
    await waitFor(() =>
      expect(ipc.commands.stopRealtime).toHaveBeenCalledTimes(1),
    );
    expect(
      await screen.findByRole("button", { name: "Start live" }),
    ).toBeTruthy();
  });

  it("stays mounted and keeps live state across a reconnect glitch", async () => {
    // The bindings mock is module-level, so drop call history from the
    // earlier start/stop test before counting calls here.
    vi.mocked(ipc.commands.startRealtime).mockClear();
    vi.mocked(ipc.commands.stopRealtime).mockClear();
    render(<Dashboard locale="en" theme="default" />);
    fireEvent.click(await screen.findByRole("button", { name: "Start live" }));
    await screen.findByRole("button", { name: "Stop live" });

    // Link glitch: the backend keeps realtime armed, so the panel must not
    // unmount — gauges keep rendering the last values, live stays "on".
    act(() => {
      useConnectionStore.setState({
        connectionState: { type: "reconnecting", attempt: 1 },
      });
    });
    expect(screen.getByRole("button", { name: "Stop live" })).toBeTruthy();
    expect(screen.getByRole("img", { name: "Engine Speed" })).toBeTruthy();

    // Recovery: same panel instance, so stopping is still a single click and
    // no second startRealtime was issued.
    act(() => {
      useConnectionStore.setState({
        connectionState: { type: "connected", signature: "sig", version: "1" },
      });
    });
    expect(screen.getByRole("button", { name: "Stop live" })).toBeTruthy();
    expect(ipc.commands.startRealtime).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Stop live" }));
    await waitFor(() =>
      expect(ipc.commands.stopRealtime).toHaveBeenCalledTimes(1),
    );
  });

  it("unmounts the panel when the connection is lost for good", async () => {
    const { container } = render(<Dashboard locale="en" theme="default" />);
    await screen.findByRole("img", { name: "Engine Speed" });
    act(() => {
      useConnectionStore.setState({
        connectionState: { type: "disconnected" },
      });
    });
    expect(container.firstChild).toBeNull();
  });

  it("rebinds a slot in edit mode and persists via saveLayout", async () => {
    render(<Dashboard locale="en" theme="default" />);
    await screen.findByRole("img", { name: "Engine Speed" });
    fireEvent.click(screen.getByRole("button", { name: "Edit layout" }));

    const binders = screen.getAllByLabelText("Bind gauge");
    expect(binders).toHaveLength(2);
    fireEvent.change(binders[0], { target: { value: "cltGauge" } });

    fireEvent.click(screen.getByRole("button", { name: "Save layout" }));
    await waitFor(() =>
      expect(ipc.commands.saveLayout).toHaveBeenCalledWith(
        serializeLayout([
          { gauge: "cltGauge", kind: "round" },
          { gauge: "cltGauge", kind: "round" },
        ]),
      ),
    );
    // Edit mode closes after a successful save. The save handler awaits the
    // saveLayout promise before calling setEditing(false), so this is a
    // separate, later DOM update than the saveLayout call above — assert it
    // with its own waitFor rather than assuming it's already flushed.
    await waitFor(() =>
      expect(screen.queryByLabelText("Bind gauge")).toBeNull(),
    );
  });

  it("reorders slots with the move buttons", async () => {
    render(<Dashboard locale="en" theme="default" />);
    await screen.findByRole("img", { name: "Engine Speed" });
    fireEvent.click(screen.getByRole("button", { name: "Edit layout" }));

    fireEvent.click(screen.getAllByRole("button", { name: "Move down" })[0]);
    const binders = screen.getAllByLabelText(
      "Bind gauge",
    ) as HTMLSelectElement[];
    expect(binders.map((b) => b.value)).toEqual(["cltGauge", "rpmGauge"]);
  });

  it("changes a slot's gauge style in edit mode", async () => {
    render(<Dashboard locale="en" theme="default" />);
    await screen.findByRole("img", { name: "Engine Speed" });
    fireEvent.click(screen.getByRole("button", { name: "Edit layout" }));

    fireEvent.change(screen.getAllByLabelText("Gauge style")[0], {
      target: { value: "digital" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save layout" }));
    await waitFor(() =>
      expect(ipc.commands.saveLayout).toHaveBeenCalledWith(
        serializeLayout([
          { gauge: "rpmGauge", kind: "digital" },
          { gauge: "cltGauge", kind: "round" },
        ]),
      ),
    );
  });

  it("shows the empty message when the INI defines no gauges", async () => {
    useTuneStore.setState({
      definition: definition({
        gauges: [],
        frontpage: { gauge_slots: [], indicators: [] },
      }),
    });
    render(<Dashboard locale="en" theme="default" />);
    expect(
      await screen.findByText("No gauges defined by this INI"),
    ).toBeTruthy();
  });
});
