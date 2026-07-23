// SPDX-License-Identifier: GPL-3.0-or-later
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  getAiSettings: vi.fn(),
  setAiSettings: vi.fn(),
  setAiKey: vi.fn(),
  clearAiKey: vi.fn(),
  aiKeyPresent: vi.fn(),
  mcpStatus: vi.fn(),
  mcpTokenInfo: vi.fn(),
}));

vi.mock("../../ipc/bindings", () => ({ commands: mocks }));

import { AiSettingsPanel } from "./AiSettingsPanel";

const ok = (data: unknown) => Promise.resolve({ status: "ok", data });

const writeText = vi.fn().mockResolvedValue(undefined);

beforeEach(() => {
  vi.clearAllMocks();
  mocks.getAiSettings.mockReturnValue(
    ok({
      enabled: false,
      provider: "anthropic",
      model: "claude-sonnet-5",
      mcpEnabled: false,
      mcpPort: 4123,
    }),
  );
  mocks.aiKeyPresent.mockReturnValue(ok(false));
  mocks.setAiSettings.mockReturnValue(ok(null));
  mocks.setAiKey.mockReturnValue(ok(null));
  mocks.clearAiKey.mockReturnValue(ok(null));
  mocks.mcpStatus.mockReturnValue(ok({ running: false, port: 0 }));
  mocks.mcpTokenInfo.mockReturnValue(ok("test-mcp-token"));
  writeText.mockClear().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", {
    value: { writeText },
    configurable: true,
  });
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

  describe("MCP section", () => {
    it("saves MCP enable and port through the extended settings DTO", async () => {
      const user = userEvent.setup();
      render(<AiSettingsPanel locale="en" />);
      await screen.findByLabelText("Enable AI (opt-in)");

      await user.click(screen.getByLabelText("Expose tools over MCP (local)"));
      const portField = screen.getByLabelText("Port");
      await user.clear(portField);
      await user.type(portField, "8765");
      await user.click(screen.getByRole("button", { name: "Save settings" }));

      await waitFor(() =>
        expect(mocks.setAiSettings).toHaveBeenCalledWith(
          expect.objectContaining({ mcpEnabled: true, mcpPort: 8765 }),
        ),
      );
    });

    it("shows the running status with the real port", async () => {
      mocks.mcpStatus.mockReturnValue(ok({ running: true, port: 8765 }));
      render(<AiSettingsPanel locale="en" />);
      expect(
        await screen.findByText("Running on port 8765"),
      ).toBeInTheDocument();
    });

    it("shows the stopped status when the server is not running", async () => {
      render(<AiSettingsPanel locale="en" />);
      expect(await screen.findByText("Stopped")).toBeInTheDocument();
    });

    it("masks the token by default and does not fetch it on mount", async () => {
      render(<AiSettingsPanel locale="en" />);
      await screen.findByLabelText("Enable AI (opt-in)");
      expect(screen.getByText("••••")).toBeInTheDocument();
      expect(mocks.mcpTokenInfo).not.toHaveBeenCalled();
    });

    it("fetches and reveals the token only when Show is clicked", async () => {
      const user = userEvent.setup();
      render(<AiSettingsPanel locale="en" />);
      await screen.findByLabelText("Enable AI (opt-in)");

      expect(mocks.mcpTokenInfo).not.toHaveBeenCalled();
      await user.click(screen.getByRole("button", { name: "Show" }));

      await waitFor(() =>
        expect(mocks.mcpTokenInfo).toHaveBeenCalledWith(false),
      );
      expect(await screen.findByText("test-mcp-token")).toBeInTheDocument();
    });

    it("copies the fetched token to the clipboard without changing the masked display", async () => {
      // NOTE: uses fireEvent, not userEvent — userEvent.setup() installs its
      // own Clipboard stub on navigator.clipboard (see
      // @testing-library/user-event's attachClipboardStubToView), which would
      // silently replace the writeText mock configured above. TableEditor's
      // Ctrl+C/Ctrl+V clipboard tests hit the same conflict and use
      // fireEvent for the same reason.
      render(<AiSettingsPanel locale="en" />);
      await screen.findByLabelText("Enable AI (opt-in)");

      fireEvent.click(screen.getByRole("button", { name: "Copy" }));

      await waitFor(() =>
        expect(mocks.mcpTokenInfo).toHaveBeenCalledWith(false),
      );
      await waitFor(() =>
        expect(writeText).toHaveBeenCalledWith("test-mcp-token"),
      );
      expect(await screen.findByText("Copied")).toBeInTheDocument();
      expect(screen.getByText("••••")).toBeInTheDocument();
    });

    it("renders the Claude Code hint with the real port and a masked token", async () => {
      mocks.mcpStatus.mockReturnValue(ok({ running: true, port: 8765 }));
      render(<AiSettingsPanel locale="en" />);
      expect(
        await screen.findByText((text) =>
          text.includes(
            'claude mcp add --transport http opentune http://127.0.0.1:8765/mcp --header "Authorization: Bearer ••••"',
          ),
        ),
      ).toBeInTheDocument();
    });

    it("regenerates the token and warns that MCP clients must be updated", async () => {
      const user = userEvent.setup();
      mocks.mcpTokenInfo.mockReturnValue(ok("rotated-token"));
      render(<AiSettingsPanel locale="en" />);
      await screen.findByLabelText("Enable AI (opt-in)");

      await user.click(screen.getByRole("button", { name: "Regenerate" }));

      await waitFor(() =>
        expect(mocks.mcpTokenInfo).toHaveBeenCalledWith(true),
      );
      expect(
        await screen.findByText("New token — update your MCP clients"),
      ).toBeInTheDocument();
    });
  });
});
