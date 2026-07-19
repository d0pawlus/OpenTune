// SPDX-License-Identifier: GPL-3.0-or-later
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { UpdateNotice } from "./UpdateNotice";

vi.mock("@tauri-apps/plugin-updater", () => ({
  check: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-process", () => ({
  relaunch: vi.fn(),
}));

function availableUpdate() {
  return {
    version: "0.3.0",
    body: "Safer tune backups",
    downloadAndInstall: vi.fn(async () => undefined),
  };
}

describe("UpdateNotice", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(relaunch).mockResolvedValue(undefined);
  });

  it("keeps a manual check action when startup finds no update", async () => {
    vi.mocked(check).mockResolvedValue(null);

    render(<UpdateNotice locale="en" />);

    await waitFor(() => expect(check).toHaveBeenCalledOnce());
    expect(
      screen.getByRole("button", { name: "Check for updates" }),
    ).toBeTruthy();
  });

  it("shows the available version and release notes", async () => {
    vi.mocked(check).mockResolvedValue(availableUpdate() as never);

    render(<UpdateNotice locale="en" />);

    expect(await screen.findByText(/0\.3\.0/)).toBeTruthy();
    expect(screen.getByText("Safer tune backups")).toBeTruthy();
  });

  it("downloads, installs, and relaunches only after explicit approval", async () => {
    const update = availableUpdate();
    vi.mocked(check).mockResolvedValue(update as never);

    render(<UpdateNotice locale="en" />);
    const install = await screen.findByRole("button", {
      name: "Install and restart",
    });

    expect(update.downloadAndInstall).not.toHaveBeenCalled();
    expect(relaunch).not.toHaveBeenCalled();
    fireEvent.click(install);

    await waitFor(() =>
      expect(update.downloadAndInstall).toHaveBeenCalledOnce(),
    );
    expect(relaunch).toHaveBeenCalledOnce();
  });

  it("renders a retryable alert when a check fails", async () => {
    vi.mocked(check).mockRejectedValue(new Error("offline"));

    render(<UpdateNotice locale="en" />);

    expect((await screen.findByRole("alert")).textContent).toBe(
      "Could not check for updates.",
    );
    expect(screen.getByRole("button", { name: "Retry" })).toBeTruthy();
  });

  it("retries a failed check and can recover", async () => {
    vi.mocked(check)
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValueOnce(null);

    render(<UpdateNotice locale="en" />);
    fireEvent.click(await screen.findByRole("button", { name: "Retry" }));

    await waitFor(() => expect(check).toHaveBeenCalledTimes(2));
    expect(screen.queryByRole("alert")).toBeNull();
    expect(
      screen.getByRole("button", { name: "Check for updates" }),
    ).toBeTruthy();
  });
});
