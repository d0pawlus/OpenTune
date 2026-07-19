// SPDX-License-Identifier: GPL-3.0-or-later
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AiStreamEvent } from "../../ipc/bindings";

const mocks = vi.hoisted(() => ({
  aiSend: vi.fn(),
  aiCancel: vi.fn(),
  aiReset: vi.fn(),
  listen: vi.fn(),
}));

vi.mock("../../ipc/bindings", () => ({
  commands: {
    aiSend: mocks.aiSend,
    aiCancel: mocks.aiCancel,
    aiReset: mocks.aiReset,
  },
  events: {
    aiStreamEvent: { listen: mocks.listen },
  },
}));

import { AiChatPanel, DELTA_FLUSH_MS } from "./AiChatPanel";

const ok = () => Promise.resolve({ status: "ok", data: null });

// The mocked `listen` captures the callback the component subscribed with,
// so a test can drive the stream by calling it directly — mirroring how
// Tauri would invoke it for a real `ai-stream-event` payload.
let capturedListener: ((e: { payload: AiStreamEvent }) => void) | null = null;

function emit(payload: AiStreamEvent) {
  act(() => {
    capturedListener?.({ payload });
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  capturedListener = null;
  mocks.listen.mockImplementation(
    (cb: (e: { payload: AiStreamEvent }) => void) => {
      capturedListener = cb;
      return Promise.resolve(() => undefined);
    },
  );
  mocks.aiSend.mockReturnValue(ok());
  mocks.aiCancel.mockReturnValue(ok());
  mocks.aiReset.mockReturnValue(ok());
});

afterEach(() => {
  vi.useRealTimers();
});

function typeAndSend(text: string) {
  fireEvent.change(screen.getByRole("textbox"), { target: { value: text } });
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
}

describe("AiChatPanel", () => {
  it("calls aiSend and renders the user entry on send", () => {
    render(<AiChatPanel locale="en" />);

    typeAndSend("What should I tune?");

    expect(mocks.aiSend).toHaveBeenCalledWith("What should I tune?");
    expect(screen.getByText("You: What should I tune?")).toBeInTheDocument();
  });

  it("buffers Delta events and only renders them after the flush interval", () => {
    vi.useFakeTimers();
    render(<AiChatPanel locale="en" />);
    typeAndSend("hi");

    emit({ kind: "delta", text: "Hel" });
    emit({ kind: "delta", text: "lo!" });

    // Still buffered — no setState per delta.
    expect(screen.queryByText("Hello!")).not.toBeInTheDocument();

    act(() => {
      vi.advanceTimersByTime(DELTA_FLUSH_MS);
    });

    expect(screen.getByText("Hello!")).toBeInTheDocument();
  });

  it("renders a chip with the tool name on ToolEnd", () => {
    render(<AiChatPanel locale="en" />);
    typeAndSend("check the table");

    emit({ kind: "toolStart", name: "read_table" });
    emit({
      kind: "toolEnd",
      name: "read_table",
      ok: true,
      summary: "VE table read",
    });

    expect(screen.getByText("read_table")).toBeInTheDocument();
    expect(screen.getByText("tool")).toBeInTheDocument();
    expect(screen.getByText("VE table read")).toBeInTheDocument();
  });

  it("surfaces an Error event via role=alert", () => {
    render(<AiChatPanel locale="en" />);
    typeAndSend("hi");

    emit({ kind: "error", message: "provider unavailable" });

    expect(screen.getByRole("alert")).toHaveTextContent("provider unavailable");
  });

  it("shows Cancel only while running and calls aiCancel when clicked", () => {
    render(<AiChatPanel locale="en" />);

    expect(
      screen.queryByRole("button", { name: "Cancel" }),
    ).not.toBeInTheDocument();

    typeAndSend("hi");

    const cancelButton = screen.getByRole("button", { name: "Cancel" });
    fireEvent.click(cancelButton);
    expect(mocks.aiCancel).toHaveBeenCalled();

    emit({ kind: "done" });
    expect(
      screen.queryByRole("button", { name: "Cancel" }),
    ).not.toBeInTheDocument();
  });
});
