// SPDX-License-Identifier: GPL-3.0-or-later
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Onboarding } from "./Onboarding";

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn(async () => undefined),
}));

const baseProps = {
  locale: "en" as const,
  theme: "default" as const,
  onLocaleChange: vi.fn(),
  onThemeChange: vi.fn(),
  onComplete: vi.fn(),
};

describe("Onboarding", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("is absent when closed and labelled as a dialog when open", () => {
    const { rerender } = render(<Onboarding {...baseProps} open={false} />);
    expect(screen.queryByRole("dialog")).toBeNull();

    rerender(<Onboarding {...baseProps} open />);

    expect(
      screen.getByRole("dialog", { name: "Welcome to OpenTune" }),
    ).toBeTruthy();
  });

  it("focuses the first action and wraps Tab in both directions", async () => {
    render(<Onboarding {...baseProps} open />);
    const first = screen.getByRole("button", { name: "English" });
    const last = screen.getByRole("button", { name: "Start using OpenTune" });
    const dialog = screen.getByRole("dialog");

    await waitFor(() => expect(document.activeElement).toBe(first));
    last.focus();
    fireEvent.keyDown(dialog, { key: "Tab" });
    expect(document.activeElement).toBe(first);

    first.focus();
    fireEvent.keyDown(dialog, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(last);
  });

  it("completes on Escape", () => {
    render(<Onboarding {...baseProps} open />);

    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });

    expect(baseProps.onComplete).toHaveBeenCalledOnce();
  });

  it("reports locale and theme choices", () => {
    render(<Onboarding {...baseProps} open />);

    fireEvent.click(screen.getByRole("button", { name: "Polski" }));
    fireEvent.click(screen.getByRole("button", { name: "High contrast" }));

    expect(baseProps.onLocaleChange).toHaveBeenCalledWith("pl");
    expect(baseProps.onThemeChange).toHaveBeenCalledWith("high-contrast");
  });

  it("opens the hosted quick start", () => {
    render(<Onboarding {...baseProps} open />);

    fireEvent.click(screen.getByRole("button", { name: "Open quick start" }));

    expect(openUrl).toHaveBeenCalledWith(
      "https://d0pawlus.github.io/OpenTune/quick-start/",
    );
  });

  it("restores focus after completion", async () => {
    function Harness() {
      const [open, setOpen] = useState(false);
      return (
        <>
          <button type="button" onClick={() => setOpen(true)}>
            Show welcome
          </button>
          <Onboarding
            {...baseProps}
            open={open}
            onComplete={() => setOpen(false)}
          />
        </>
      );
    }

    render(<Harness />);
    const opener = screen.getByRole("button", { name: "Show welcome" });
    opener.focus();
    fireEvent.click(opener);
    fireEvent.click(
      await screen.findByRole("button", { name: "Start using OpenTune" }),
    );

    await waitFor(() => expect(document.activeElement).toBe(opener));
  });
});
