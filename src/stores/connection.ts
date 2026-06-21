// SPDX-License-Identifier: GPL-3.0-or-later
import { create } from "zustand";

interface ConnectionState {
  lastSeq: number | null;
  setSeq: (n: number) => void;
}

export const useConnectionStore = create<ConnectionState>((set) => ({
  lastSeq: null,
  setSeq: (n) => set({ lastSeq: n }),
}));
