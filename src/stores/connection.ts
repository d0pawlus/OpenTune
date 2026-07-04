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

/**
 * Whether the link is alive enough for connection-scoped frontend state to
 * stay mounted across a transient glitch: `connected` and `reconnecting`
 * both count as alive, because the backend deliberately keeps realtime
 * polling and tune state armed through a link drop (`Reconnecting` only ever
 * follows `Connected` — see `ConnectionState` in
 * `src-tauri/crates/protocol/src/lib.rs`). Only the terminal
 * `disconnected`/`connecting`/`failed` states are not alive.
 *
 * Shared by every consumer whose mounted/store state must survive a
 * `reconnecting` blip rather than just a full disconnect (`Dashboard`,
 * `TunePanel`) — a single predicate here keeps their link-alive semantics in
 * sync instead of each re-deriving its own variant.
 */
export function isLinkAlive(
  state: ConnectionStateEvent | null | undefined,
): boolean {
  return state?.type === "connected" || state?.type === "reconnecting";
}
