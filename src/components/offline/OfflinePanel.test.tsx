// SPDX-License-Identifier: GPL-3.0-or-later
import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { OfflinePanel } from "./OfflinePanel";
import { useTuneStore } from "../../stores/tune";
import type { DefinitionDto } from "../../ipc/bindings";

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(async () => "/tmp/my.ini"),
  save: vi.fn(async () => "/tmp/out.msq"),
}));
vi.mock("../../ipc/bindings", () => ({
  commands: {
    newTune: vi.fn(async () => ({
      status: "ok",
      data: {
        signature: "s",
        menus: [{ label: "Fuel", items: [{ label: "Fuel", dialog: "fuel" }] }],
        dialogs: [{ name: "fuel", title: "Fuel", fields: [] }],
        constants: [],
        tables: [],
        curves: [],
        gauges: [],
        frontpage: { gauge_slots: [], indicators: [] },
      },
    })),
    openTune: vi.fn(async () => ({
      status: "ok",
      data: {
        signature: "s",
        menus: [],
        dialogs: [],
        constants: [],
        tables: [],
        curves: [],
        gauges: [],
        frontpage: { gauge_slots: [], indicators: [] },
      },
    })),
    saveTune: vi.fn(async () => ({ status: "ok", data: null })),
    writeTuneToEcu: vi.fn(async () => ({ status: "ok", data: null })),
  },
}));

const emptyDefinition: DefinitionDto = {
  signature: "s",
  menus: [],
  dialogs: [],
  constants: [],
  tables: [],
  curves: [],
  gauges: [],
  frontpage: { gauge_slots: [], indicators: [] },
};

describe("OfflinePanel", () => {
  beforeEach(() => {
    useTuneStore.getState().reset();
    vi.clearAllMocks();
  });

  it("new tune picks an INI and loads an offline definition", async () => {
    const { commands } = await import("../../ipc/bindings");
    render(<OfflinePanel locale="en" />);
    fireEvent.click(screen.getByText(/new tune/i));
    await vi.waitFor(() =>
      expect(commands.newTune).toHaveBeenCalledWith("/tmp/my.ini"),
    );
    await vi.waitFor(() => expect(useTuneStore.getState().offline).toBe(true));
    expect(useTuneStore.getState().activeDialog).toBe("fuel");
  });

  it("open tune picks an INI then a .msq and loads an offline definition", async () => {
    const { commands } = await import("../../ipc/bindings");
    render(<OfflinePanel locale="en" />);
    fireEvent.click(screen.getByText(/open tune/i));
    await vi.waitFor(() =>
      expect(commands.openTune).toHaveBeenCalledWith(
        "/tmp/my.ini",
        "/tmp/my.ini",
      ),
    );
    await vi.waitFor(() => expect(useTuneStore.getState().offline).toBe(true));
  });

  it("save is disabled until a tune is loaded, then writes via commands.saveTune", async () => {
    const { commands } = await import("../../ipc/bindings");
    render(<OfflinePanel locale="en" />);
    const saveButton = screen.getByText(/^save tune$/i) as HTMLButtonElement;
    expect(saveButton.disabled).toBe(true);

    act(() => {
      useTuneStore.getState().setOfflineDefinition(emptyDefinition);
    });
    expect(saveButton.disabled).toBe(false);

    fireEvent.click(saveButton);
    await vi.waitFor(() =>
      expect(commands.saveTune).toHaveBeenCalledWith("/tmp/out.msq"),
    );
  });

  it("surfaces an error and does not load a definition when newTune fails", async () => {
    const { commands } = await import("../../ipc/bindings");
    vi.mocked(commands.newTune).mockResolvedValueOnce({
      status: "error",
      error: "bad ini",
    });
    render(<OfflinePanel locale="en" />);
    fireEvent.click(screen.getByText(/new tune/i));
    await screen.findByText("bad ini");
    expect(useTuneStore.getState().offline).toBe(false);
  });
});
