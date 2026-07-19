// SPDX-License-Identifier: GPL-3.0-or-later
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, afterEach } from "vitest";
import { ProposalCard } from "./ProposalCard";
import { useTuneStore } from "../../stores/tune";
import type { AiProposalDto } from "../../ipc/bindings";

// `setCells` is what actually reaches the backend — spy on the real store's
// action (the exact call `AutoTunePanel.apply` uses) rather than mocking the
// whole zustand module, so the Apply gating logic exercises the real
// `useTuneStore.getState()` lookup ProposalCard performs.
afterEach(() => {
  vi.restoreAllMocks();
});

const okProposal: AiProposalDto = {
  id: 1,
  constant: "reqFuel",
  reason: "Richen slightly under load to reduce lean surge.",
  ok: true,
  cells: [{ index: 0, value: 13, ok: true, note: null }],
  edits: [{ index: 0, value: 13 }],
};

const notOkProposal: AiProposalDto = {
  id: 2,
  constant: "reqFuel",
  reason: "Would exceed the guardrail delta limit.",
  ok: false,
  cells: [
    {
      index: 0,
      value: 40,
      ok: false,
      note: "change of 60.0% exceeds the 20.0% limit",
    },
  ],
  edits: [],
};

describe("ProposalCard", () => {
  it("ok proposal: Apply is enabled, calls setCells with the exact edits, and shows applied status", async () => {
    const setCells = vi
      .spyOn(useTuneStore.getState(), "setCells")
      .mockResolvedValue(undefined);
    const onApplied = vi.fn();

    render(
      <ProposalCard proposal={okProposal} locale="en" onApplied={onApplied} />,
    );

    const applyButton = screen.getByRole("button", { name: "Apply" });
    expect(applyButton).toBeEnabled();

    fireEvent.click(applyButton);

    await waitFor(() =>
      expect(setCells).toHaveBeenCalledWith("reqFuel", [
        { index: 0, value: 13 },
      ]),
    );
    await waitFor(() =>
      expect(screen.getByText("Applied")).toBeInTheDocument(),
    );
    expect(onApplied).toHaveBeenCalled();
  });

  it("not-ok proposal: Apply is disabled and the invalid note is visible", () => {
    render(
      <ProposalCard proposal={notOkProposal} locale="en" onApplied={vi.fn()} />,
    );

    expect(screen.getByRole("button", { name: "Apply" })).toBeDisabled();
    expect(
      screen.getByText("Not applicable — failed validation"),
    ).toBeInTheDocument();
  });

  it("surfaces a setCells rejection via role=alert", async () => {
    vi.spyOn(useTuneStore.getState(), "setCells").mockRejectedValue(
      new Error("out of range"),
    );

    render(
      <ProposalCard proposal={okProposal} locale="en" onApplied={vi.fn()} />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent("out of range"),
    );
  });

  it("Dismiss hides the card", () => {
    render(
      <ProposalCard proposal={okProposal} locale="en" onApplied={vi.fn()} />,
    );

    expect(screen.getByText("reqFuel")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));

    expect(screen.queryByText("reqFuel")).not.toBeInTheDocument();
  });
});
