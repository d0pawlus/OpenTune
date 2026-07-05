// SPDX-License-Identifier: GPL-3.0-or-later
import { create } from "zustand";
import type { RealtimeFrameEvent } from "../ipc/bindings";

/**
 * Reflect-only realtime channel store.
 *
 * Frames arrive at ≤30 Hz; canvas gauges read this store **imperatively**
 * via `useRealtimeStore.getState().getChannel(...)` inside their
 * `requestAnimationFrame` paint loop — never through React selector
 * subscriptions — so frame updates never enter React reconciliation.
 *
 * The store never computes physics: values are reflected verbatim from the
 * backend-decoded frame (the backend is the single source of truth).
 */
interface RealtimeStore {
  /** Latest known physical value per output-channel name. */
  channels: Record<string, number>;
  /** Reflect one `RealtimeFrameEvent` into the channel map. */
  applyFrame: (frame: RealtimeFrameEvent) => void;
  /** Imperative read for the rAF paint loop; `undefined` = never seen. */
  getChannel: (name: string) => number | undefined;
  /** Drop every channel (on disconnect / stop). */
  clear: () => void;
}

export const useRealtimeStore = create<RealtimeStore>((set, get) => ({
  channels: {},

  applyFrame: (frame) => {
    const channels = { ...get().channels };
    for (const [name, value] of frame.channels) {
      // Fail-open per item: an unresolvable channel decodes to NaN on the
      // backend, which serde_json serializes to JSON `null`. Skip it so the
      // bound gauge keeps its neutral state instead of showing a bogus 0.
      if (value !== null && Number.isFinite(value)) {
        channels[name] = value;
      }
    }
    set({ channels });
  },

  getChannel: (name) => get().channels[name],

  clear: () => set({ channels: {} }),
}));
