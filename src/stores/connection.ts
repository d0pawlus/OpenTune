// SPDX-License-Identifier: GPL-3.0-or-later
import { create } from "zustand";
import type { ConnectionStateEvent } from "../ipc/bindings";

interface ConnectionStore {
  /** Raw heartbeat sequence number (M0 heartbeat demo). */
  lastSeq: number | null;
  setSeq: (n: number) => void;

  /**
   * Current connection state as reported by the backend via the
   * `connection-state-event` IPC event.  Starts `null` (no event received
   * yet) so the UI can distinguish "not yet known" from "Disconnected".
   */
  connectionState: ConnectionStateEvent | null;

  /**
   * Apply a state transition emitted by the backend.  This is the single
   * place the frontend updates connection state — keeps the backend as the
   * source of truth.
   */
  applyConnectionState: (event: ConnectionStateEvent) => void;
}

export const useConnectionStore = create<ConnectionStore>((set) => ({
  lastSeq: null,
  setSeq: (n) => set({ lastSeq: n }),

  connectionState: null,
  applyConnectionState: (event) => set({ connectionState: event }),
}));
