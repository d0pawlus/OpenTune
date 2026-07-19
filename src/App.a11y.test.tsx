// SPDX-License-Identifier: GPL-3.0-or-later
import { render, screen } from "@testing-library/react";
import axe from "axe-core";
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
  Connect: () => (
    <section aria-labelledby="mock-connect">
      <h2 id="mock-connect">Connect</h2>
    </section>
  ),
}));
vi.mock("./components/dashboard/Dashboard", () => ({
  Dashboard: () => (
    <section aria-labelledby="mock-dashboard">
      <h2 id="mock-dashboard">Dashboard</h2>
    </section>
  ),
}));
vi.mock("./components/offline/OfflinePanel", () => ({
  OfflinePanel: () => (
    <section aria-labelledby="mock-offline">
      <h2 id="mock-offline">Offline</h2>
    </section>
  ),
}));
vi.mock("./components/dialogs/TunePanel", () => ({
  TunePanel: () => (
    <section aria-labelledby="mock-tune">
      <h2 id="mock-tune">Tune</h2>
    </section>
  ),
}));
vi.mock("./components/datalog/DatalogPanel", () => ({
  DatalogPanel: () => (
    <section aria-labelledby="mock-datalog">
      <h2 id="mock-datalog">Datalog</h2>
    </section>
  ),
}));
vi.mock("./components/ai/AiSettingsPanel", () => ({
  AiSettingsPanel: () => (
    <section aria-labelledby="mock-ai-settings">
      <h2 id="mock-ai-settings">AI assistant</h2>
    </section>
  ),
}));
vi.mock("./components/update/UpdateNotice", () => ({
  UpdateNotice: () => (
    <section aria-labelledby="mock-update">
      <h2 id="mock-update">Application updates</h2>
      <button type="button">Check for updates</button>
    </section>
  ),
}));

async function expectNoViolations(container: HTMLElement) {
  await screen.findByText("OpenTune v0.2.0");
  const result = await axe.run(container, {
    rules: {
      // jsdom has no canvas implementation, which axe needs for computed
      // contrast. Contrast remains an explicit manual M6 check.
      "color-contrast": { enabled: false },
    },
  });
  expect(result.violations.map(({ id, help }) => ({ id, help }))).toEqual([]);
}

describe("App accessibility smoke", () => {
  beforeEach(() => {
    localStorage.clear();
    Object.defineProperty(window.navigator, "language", {
      configurable: true,
      value: "en-US",
    });
  });

  it("has no automated violations on first run", async () => {
    const { container } = render(<App />);
    expect(screen.getByRole("dialog")).toBeTruthy();

    await expectNoViolations(container);
  });

  it("has no automated violations in the completed shell", async () => {
    localStorage.setItem("opentune.onboarding.v1", "complete");
    const { container } = render(<App />);
    expect(screen.queryByRole("dialog")).toBeNull();

    await expectNoViolations(container);
  });
});
