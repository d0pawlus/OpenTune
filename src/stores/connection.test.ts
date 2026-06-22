// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, it, expect, beforeEach } from "vitest";
import { useConnectionStore } from "./connection";
import type { ConnectionStateEvent } from "../ipc/bindings";

describe("connection store", () => {
  beforeEach(() => {
    // Reset store state between tests so they are independent.
    useConnectionStore.setState({ lastSeq: null, connectionState: null });
  });

  it("starts with no sequence and records the latest", () => {
    expect(useConnectionStore.getState().lastSeq).toBeNull();
    useConnectionStore.getState().setSeq(7);
    expect(useConnectionStore.getState().lastSeq).toBe(7);
  });

  it("starts with null connectionState (not yet connected)", () => {
    expect(useConnectionStore.getState().connectionState).toBeNull();
  });

  it("applyConnectionState transitions to Disconnected", () => {
    const event: ConnectionStateEvent = { type: "disconnected" };
    useConnectionStore.getState().applyConnectionState(event);
    expect(useConnectionStore.getState().connectionState).toEqual({
      type: "disconnected",
    });
  });

  it("applyConnectionState transitions to Reconnecting with attempt count", () => {
    const event: ConnectionStateEvent = { type: "reconnecting", attempt: 2 };
    useConnectionStore.getState().applyConnectionState(event);
    const state = useConnectionStore.getState().connectionState;
    expect(state).toEqual({ type: "reconnecting", attempt: 2 });
  });

  it("applyConnectionState transitions to Connected with signature", () => {
    const event: ConnectionStateEvent = {
      type: "connected",
      signature: "speeduino 202504-dev",
      version: "Speeduino 2025.04",
    };
    useConnectionStore.getState().applyConnectionState(event);
    const state = useConnectionStore.getState().connectionState;
    expect(state).toEqual({
      type: "connected",
      signature: "speeduino 202504-dev",
      version: "Speeduino 2025.04",
    });
  });

  it("applyConnectionState transitions to Failed with reason", () => {
    const event: ConnectionStateEvent = {
      type: "failed",
      reason: "reconnect failed after 10 attempts",
    };
    useConnectionStore.getState().applyConnectionState(event);
    const state = useConnectionStore.getState().connectionState;
    expect(state).toEqual({
      type: "failed",
      reason: "reconnect failed after 10 attempts",
    });
  });

  it("state transitions are independent and immutable", () => {
    // Each transition fully replaces the previous state (no partial mutation).
    useConnectionStore
      .getState()
      .applyConnectionState({ type: "reconnecting", attempt: 1 });
    useConnectionStore
      .getState()
      .applyConnectionState({ type: "connected", signature: "sig", version: "v" });
    expect(useConnectionStore.getState().connectionState).toEqual({
      type: "connected",
      signature: "sig",
      version: "v",
    });
  });

  it("handles the full lifecycle: connecting → connected → reconnecting → connected", () => {
    // Simulates a connection drop and recovery.
    useConnectionStore
      .getState()
      .applyConnectionState({ type: "connecting" });
    expect(useConnectionStore.getState().connectionState?.type).toBe(
      "connecting",
    );

    useConnectionStore.getState().applyConnectionState({
      type: "connected",
      signature: "speeduino",
      version: "2025.04",
    });
    expect(useConnectionStore.getState().connectionState?.type).toBe(
      "connected",
    );

    // Link drops.
    useConnectionStore
      .getState()
      .applyConnectionState({ type: "reconnecting", attempt: 1 });
    expect(useConnectionStore.getState().connectionState?.type).toBe(
      "reconnecting",
    );

    // Recover.
    useConnectionStore.getState().applyConnectionState({
      type: "connected",
      signature: "speeduino",
      version: "2025.04",
    });
    expect(useConnectionStore.getState().connectionState?.type).toBe(
      "connected",
    );
  });

  it("stores signature and version separately from connection type", () => {
    const connectedEvent: ConnectionStateEvent = {
      type: "connected",
      signature: "speeduino 202504-dev",
      version: "Speeduino 2025.04",
    };
    useConnectionStore.getState().applyConnectionState(connectedEvent);
    const state = useConnectionStore.getState().connectionState;
    if (state?.type === "connected") {
      expect(state.signature).toBe("speeduino 202504-dev");
      expect(state.version).toBe("Speeduino 2025.04");
    } else {
      throw new Error("Expected connected state");
    }
  });
});
