// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, it, expect, beforeEach, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import { Connect } from "./Connect";
import { useConnectionStore } from "../stores/connection";
import * as ipc from "../ipc/bindings";

// Mock the IPC module
vi.mock("../ipc/bindings", () => ({
  commands: {
    listPorts: vi.fn(),
  },
  events: {
    connectionStateEvent: {
      listen: vi.fn(() => Promise.resolve(() => {})),
    },
    heartbeat: {
      listen: vi.fn(() => Promise.resolve(() => {})),
    },
  },
}));

describe("Connect component", () => {
  beforeEach(() => {
    useConnectionStore.setState({
      connectionState: null,
      lastSeq: null,
    });
  });

  it("renders the connect section with port selection", async () => {
    const mockListPorts = vi.mocked(ipc.commands.listPorts);
    mockListPorts.mockResolvedValue({
      status: "ok",
      data: [
        { name: "COM3", vid: 0x2341, pid: 0x0043, product: "Arduino Uno" },
        { name: "/dev/ttyUSB0", vid: null, pid: null, product: null },
      ],
    });

    render(<Connect locale="en" />);

    await waitFor(() => {
      const heading = screen.getByText("Connect to ECU");
      expect(heading).toBeTruthy();
    });

    const select = screen.getByDisplayValue("COM3 (Arduino Uno)") as HTMLSelectElement;
    expect(select).toBeTruthy();
  });

  it("disables connect button when no port is selected", async () => {
    const mockListPorts = vi.mocked(ipc.commands.listPorts);
    mockListPorts.mockResolvedValue({ status: "ok", data: [] as any[] });

    render(<Connect locale="en" />);

    await waitFor(() => {
      const buttons = screen.getAllByRole("button");
      const connectButton = buttons.find((b) => b.textContent === "Connect");
      expect(connectButton?.hasAttribute("disabled")).toBeTruthy();
    });
  });

  it("displays connection state when connected", async () => {
    const mockListPorts = vi.mocked(ipc.commands.listPorts);
    mockListPorts.mockResolvedValue({
      status: "ok",
      data: [{ name: "COM3", vid: null, pid: null, product: null }],
    });

    useConnectionStore.setState({
      connectionState: {
        type: "connected",
        signature: "speeduino 202504",
        version: "Speeduino 2025.04",
      },
    });

    render(<Connect locale="en" />);

    await waitFor(() => {
      expect(screen.getByText(/speeduino 202504/)).toBeTruthy();
      expect(screen.getByText(/Speeduino 2025.04/)).toBeTruthy();
    });
  });

  it("displays reconnecting state with attempt count", async () => {
    const mockListPorts = vi.mocked(ipc.commands.listPorts);
    mockListPorts.mockResolvedValue({
      status: "ok",
      data: [{ name: "COM3", vid: null, pid: null, product: null }],
    });

    useConnectionStore.setState({
      connectionState: {
        type: "reconnecting",
        attempt: 3,
      },
    });

    render(<Connect locale="en" />);

    await waitFor(() => {
      // Check that the reconnecting state is displayed
      const text = screen.getByText(/Reconnecting/);
      expect(text?.textContent).toContain("Reconnecting");
      expect(text?.textContent).toContain("3");
    });
  });

  it("handles empty port list gracefully", async () => {
    const mockListPorts = vi.mocked(ipc.commands.listPorts);
    mockListPorts.mockResolvedValue({ status: "ok", data: [] as any[] });

    render(<Connect locale="en" />);

    await waitFor(() => {
      expect(
        screen.getByText("No serial ports available")
      ).toBeTruthy();
    });
  });

  it("uses Polish i18n when locale is pl", async () => {
    const mockListPorts = vi.mocked(ipc.commands.listPorts);
    mockListPorts.mockResolvedValue({ status: "ok", data: [] as any[] });

    render(<Connect locale="pl" />);

    await waitFor(() => {
      expect(screen.getByText("Połącz z ECU")).toBeTruthy();
      expect(
        screen.getByText("Brak dostępnych portów szeregowych")
      ).toBeTruthy();
    });
  });
});
