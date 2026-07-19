// SPDX-License-Identifier: GPL-3.0-or-later
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  getAiSettings: vi.fn(),
  setAiSettings: vi.fn(),
  setAiKey: vi.fn(),
  clearAiKey: vi.fn(),
  aiKeyPresent: vi.fn(),
}));

vi.mock("../../ipc/bindings", () => ({ commands: mocks }));

import { AiSettingsPanel } from "./AiSettingsPanel";

const ok = (data: unknown) => Promise.resolve({ status: "ok", data });

beforeEach(() => {
  vi.clearAllMocks();
  mocks.getAiSettings.mockReturnValue(
    ok({ enabled: false, provider: "anthropic", model: "claude-sonnet-5" }),
  );
  mocks.aiKeyPresent.mockReturnValue(ok(false));
  mocks.setAiSettings.mockReturnValue(ok(null));
  mocks.setAiKey.mockReturnValue(ok(null));
  mocks.clearAiKey.mockReturnValue(ok(null));
});

describe("AiSettingsPanel", () => {
  it("loads settings and shows key-missing status", async () => {
    render(<AiSettingsPanel locale="en" />);
    await waitFor(() => expect(mocks.getAiSettings).toHaveBeenCalled());
    expect(await screen.findByText("No API key saved")).toBeInTheDocument();
    expect(screen.getByLabelText("Enable AI (opt-in)")).not.toBeChecked();
  });

  it("saves the key write-only and clears the field", async () => {
    const user = userEvent.setup();
    mocks.aiKeyPresent.mockReturnValueOnce(ok(false)).mockReturnValue(ok(true));
    render(<AiSettingsPanel locale="en" />);
    const field = await screen.findByLabelText("API key");
    await user.type(field, "test-key");
    await user.click(screen.getByRole("button", { name: "Save key" }));
    await waitFor(() =>
      expect(mocks.setAiKey).toHaveBeenCalledWith("anthropic", "test-key"),
    );
    expect((field as HTMLInputElement).value).toBe("");
    expect(await screen.findByText("API key saved")).toBeInTheDocument();
  });

  it("persists settings changes", async () => {
    const user = userEvent.setup();
    render(<AiSettingsPanel locale="en" />);
    await screen.findByLabelText("Enable AI (opt-in)");
    await user.click(screen.getByLabelText("Enable AI (opt-in)"));
    await user.click(screen.getByRole("button", { name: "Save settings" }));
    await waitFor(() =>
      expect(mocks.setAiSettings).toHaveBeenCalledWith(
        expect.objectContaining({ enabled: true, provider: "anthropic" }),
      ),
    );
  });

  it("surfaces command errors via role=alert", async () => {
    mocks.getAiSettings.mockReturnValue(
      Promise.resolve({ status: "error", error: "boom" }),
    );
    render(<AiSettingsPanel locale="en" />);
    expect(await screen.findByRole("alert")).toHaveTextContent("boom");
  });
});
