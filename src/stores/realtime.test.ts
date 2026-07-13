// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, it, expect, beforeEach } from "vitest";
import { useRealtimeStore } from "./realtime";
import type { RealtimeFrameEvent } from "../ipc/bindings";

const frame = (
  channels: RealtimeFrameEvent["channels"],
): RealtimeFrameEvent => ({ channels });

describe("realtime store (reflect-only)", () => {
  beforeEach(() => {
    useRealtimeStore.setState({ channels: {} });
  });

  it("applyFrame reflects the frame's channel pairs into the map", () => {
    useRealtimeStore.getState().applyFrame(
      frame([
        ["rpm", 3500],
        ["clt", 87.5],
      ]),
    );
    expect(useRealtimeStore.getState().getChannel("rpm")).toBe(3500);
    expect(useRealtimeStore.getState().getChannel("clt")).toBe(87.5);
  });

  it("getChannel returns undefined for a channel never seen (fail-open)", () => {
    useRealtimeStore.getState().applyFrame(frame([["rpm", 3500]]));
    expect(useRealtimeStore.getState().getChannel("map")).toBeUndefined();
  });

  it("reflects values verbatim — the store never computes physics", () => {
    // A raw-looking value must come through untouched: no scaling, no
    // translation, no rounding. Decode happens on the backend only.
    useRealtimeStore.getState().applyFrame(frame([["batteryV", 12.345678]]));
    expect(useRealtimeStore.getState().getChannel("batteryV")).toBe(12.345678);
  });

  it("skips null channel values (NaN fail-open sentinel from the backend)", () => {
    useRealtimeStore.getState().applyFrame(
      frame([
        ["rpm", 3000],
        ["broken", null],
      ]),
    );
    expect(useRealtimeStore.getState().getChannel("broken")).toBeUndefined();
    expect(useRealtimeStore.getState().getChannel("rpm")).toBe(3000);
  });

  it("later frames overwrite updated channels and keep untouched ones", () => {
    useRealtimeStore.getState().applyFrame(
      frame([
        ["rpm", 1000],
        ["clt", 80],
      ]),
    );
    useRealtimeStore.getState().applyFrame(frame([["rpm", 2000]]));
    expect(useRealtimeStore.getState().getChannel("rpm")).toBe(2000);
    expect(useRealtimeStore.getState().getChannel("clt")).toBe(80);
  });

  it("clear drops every channel", () => {
    useRealtimeStore.getState().applyFrame(frame([["rpm", 1000]]));
    useRealtimeStore.getState().clear();
    expect(useRealtimeStore.getState().getChannel("rpm")).toBeUndefined();
  });
});

// M2/M3 re-review finding 7: replaying a recorded log through the live
// `applyFrame` (skip-on-null) kept the previous gauge reading across a
// genuine null gap — while the user scrubs to diagnose that very dropout.
describe("realtime store replay rows", () => {
  beforeEach(() => {
    useRealtimeStore.setState({ channels: {} });
  });

  it("applyReplayRow clears a channel on a recorded null gap", () => {
    useRealtimeStore.getState().applyFrame(frame([["rpm", 3000]]));
    useRealtimeStore.getState().applyReplayRow(
      frame([
        ["rpm", null],
        ["clt", 80],
      ]),
    );
    expect(useRealtimeStore.getState().getChannel("rpm")).toBeUndefined();
    expect(useRealtimeStore.getState().getChannel("clt")).toBe(80);
  });

  it("applyReplayRow restores the channel when the gap ends", () => {
    useRealtimeStore.getState().applyReplayRow(frame([["rpm", null]]));
    useRealtimeStore.getState().applyReplayRow(frame([["rpm", 1500]]));
    expect(useRealtimeStore.getState().getChannel("rpm")).toBe(1500);
  });
});
