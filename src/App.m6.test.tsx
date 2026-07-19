// SPDX-License-Identifier: GPL-3.0-or-later
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";

vi.mock("./ipc/bindings", () => ({
  commands: {
    appInfo: vi.fn(async () => ({ name: "OpenTune", version: "0.2.0" })),
  },
  events: {
    heartbeat: { listen: vi.fn(async () => () => undefined) },
    connectionStateEvent: { listen: vi.fn(async () => () => undefined) },
    realtimeFrameEvent: { listen: vi.fn(async () => () => undefined) },
  },
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn(async () => undefined),
}));

vi.mock("./components/Connect", () => ({
  Connect: ({ locale }: { locale: string }) => <div>connect-{locale}</div>,
}));
vi.mock("./components/dashboard/Dashboard", () => ({
  Dashboard: ({ theme }: { theme: string }) => <div>dashboard-{theme}</div>,
}));
vi.mock("./components/offline/OfflinePanel", () => ({
  OfflinePanel: () => <div>offline</div>,
}));
vi.mock("./components/dialogs/TunePanel", () => ({
  TunePanel: () => <div>tune</div>,
}));
vi.mock("./components/datalog/DatalogPanel", () => ({
  DatalogPanel: () => <div>datalog</div>,
}));
vi.mock("./components/ai/AiSettingsPanel", () => ({
  AiSettingsPanel: () => <div>ai-settings</div>,
}));
vi.mock("./components/update/UpdateNotice", () => ({
  UpdateNotice: ({ locale }: { locale: string }) => <div>update-{locale}</div>,
}));

describe("App M6 preferences and onboarding composition", () => {
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.dataset.theme = "";
    Object.defineProperty(window.navigator, "language", {
      configurable: true,
      value: "en-US",
    });
  });

  it("persists first-run locale/theme choices and completion", async () => {
    const { container } = render(<App />);

    expect(
      screen.getByRole("dialog", { name: "Welcome to OpenTune" }),
    ).toBeTruthy();
    expect(container.querySelector("main")?.hasAttribute("inert")).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "Polski" }));
    fireEvent.click(screen.getByRole("button", { name: "Wysoki kontrast" }));
    fireEvent.click(
      screen.getByRole("button", {
        name: "Rozpocznij pracę z OpenTune",
      }),
    );

    expect(screen.queryByRole("dialog")).toBeNull();
    expect(container.querySelector("main")?.hasAttribute("inert")).toBe(false);
    expect(localStorage.getItem("opentune.locale")).toBe("pl");
    expect(localStorage.getItem("opentune.theme")).toBe("high-contrast");
    expect(localStorage.getItem("opentune.onboarding.v1")).toBe("complete");
    expect(document.documentElement.dataset.theme).toBe("high-contrast");
    expect(screen.getByText("connect-pl")).toBeTruthy();
    expect(screen.getByText("update-pl")).toBeTruthy();
  });

  it("restores saved choices and lets the footer reopen onboarding", async () => {
    localStorage.setItem("opentune.locale", "pl");
    localStorage.setItem("opentune.theme", "high-contrast");
    localStorage.setItem("opentune.onboarding.v1", "complete");

    render(<App />);

    expect(screen.queryByRole("dialog")).toBeNull();
    await waitFor(() =>
      expect(document.documentElement.dataset.theme).toBe("high-contrast"),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Pokaż przewodnik powitalny" }),
    );

    expect(
      screen.getByRole("dialog", { name: "Witaj w OpenTune" }),
    ).toBeTruthy();
  });
});
