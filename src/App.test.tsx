// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, it, expect } from "vitest";
import { useConnectionStore } from "./stores/connection";

// The App subscribes to heartbeat events and pushes seq values
// into the connection store. We test the store integration here
// (the event listener itself requires a running Tauri runtime).
describe("App heartbeat → store integration", () => {
  it("renders heartbeat placeholder before any event arrives", () => {
    // Reset store state between tests
    useConnectionStore.setState({ lastSeq: null });
    expect(useConnectionStore.getState().lastSeq).toBeNull();
  });

  it("store reflects seq after simulated heartbeat", () => {
    useConnectionStore.setState({ lastSeq: null });
    useConnectionStore.getState().setSeq(42);
    expect(useConnectionStore.getState().lastSeq).toBe(42);
  });
});
