// SPDX-License-Identifier: GPL-3.0-or-later
import { create } from "zustand";
import { commands } from "../ipc/bindings";
import type { DefinitionDto, TuneDirtyEvent, Value } from "../ipc/bindings";
import type { CellEdit } from "../components/table-editor/tableOps";

interface TuneStore {
  /** The definition being rendered, or `null` before it is loaded. */
  definition: DefinitionDto | null;
  /**
   * True when `definition` came from a file-backed offline load
   * (`setOfflineDefinition`) rather than a live ECU read (`setDefinition`).
   * An offline tune has no wire link to lose, so it survives a true
   * disconnect instead of being reset — see `TunePanel`'s reset effect.
   */
  offline: boolean;
  /** Current physical values keyed by constant name. */
  values: Record<string, Value>;
  /**
   * Whether RAM diverges from flash ("modified, not burned"). The backend is
   * the single source of truth — this only ever changes via {@link applyDirty}
   * reflecting a `tune_dirty` event, never computed on the frontend.
   */
  dirty: boolean;
  /** Page numbers with unburned edits. */
  dirtyPages: number[];
  /** The dialog currently shown, selected from a menu. */
  activeDialog: string | null;
  /** The table editor currently shown, selected from the Tables nav (M4). */
  activeTable: string | null;
  /** The curve editor currently shown, selected from the Curves nav (M4). */
  activeCurve: string | null;

  /** Loads a definition from a live ECU read; clears `offline`. */
  setDefinition: (definition: DefinitionDto | null) => void;
  /** Loads a definition from a file-backed offline open/new; sets `offline`. */
  setOfflineDefinition: (definition: DefinitionDto) => void;
  setValues: (values: Record<string, Value>) => void;
  /** Selects a dialog, clearing any active table/curve (single content pane). */
  setActiveDialog: (activeDialog: string | null) => void;
  /** Selects a table, clearing any active dialog/curve (single content pane). */
  setActiveTable: (activeTable: string | null) => void;
  /** Selects a curve, clearing any active dialog/table (single content pane). */
  setActiveCurve: (activeCurve: string | null) => void;

  /** Merges a patch of freshly-read values without dropping existing keys. */
  mergeValues: (patch: Record<string, Value>) => void;

  /** Reflect a backend `tune_dirty` event. */
  applyDirty: (event: TuneDirtyEvent) => void;

  /**
   * Optimistically set a value and write it live via the backend. The local
   * value updates immediately; if the command fails, it rolls back to the
   * previous value and rethrows so the caller can surface the error. The dirty
   * badge is driven separately by the `tune_dirty` event, not here.
   */
  setValue: (name: string, value: Value) => Promise<void>;

  /**
   * Optimistically patches an array constant's cells and writes them live via
   * the backend as a single gesture (Task 3's `set_cells` — one command, one
   * undo step). Rolls back to the previous array and rethrows on failure,
   * mirroring `setValue`.
   */
  setCells: (name: string, edits: CellEdit[]) => Promise<void>;

  reset: () => void;
}

const INITIAL: Pick<
  TuneStore,
  | "definition"
  | "offline"
  | "values"
  | "dirty"
  | "dirtyPages"
  | "activeDialog"
  | "activeTable"
  | "activeCurve"
> = {
  definition: null,
  offline: false,
  values: {},
  dirty: false,
  dirtyPages: [],
  activeDialog: null,
  activeTable: null,
  activeCurve: null,
};

export const useTuneStore = create<TuneStore>((set, get) => ({
  ...INITIAL,

  setDefinition: (definition) => set({ definition, offline: false }),
  setOfflineDefinition: (definition) => set({ definition, offline: true }),
  setValues: (values) => set({ values }),
  setActiveDialog: (activeDialog) =>
    set({ activeDialog, activeTable: null, activeCurve: null }),
  setActiveTable: (activeTable) =>
    set({ activeTable, activeDialog: null, activeCurve: null }),
  setActiveCurve: (activeCurve) =>
    set({ activeCurve, activeDialog: null, activeTable: null }),

  mergeValues: (patch) => set((s) => ({ values: { ...s.values, ...patch } })),

  applyDirty: (event) =>
    set({ dirty: event.dirty, dirtyPages: event.dirty_pages }),

  setValue: async (name, value) => {
    const previous = get().values[name];
    // Optimistic update.
    set((s) => ({ values: { ...s.values, [name]: value } }));

    const result = await commands.setValue(name, value);
    if (result.status === "error") {
      // Roll back: restore the prior value, or drop the key if there was none.
      set((s) => {
        const values = { ...s.values };
        if (previous === undefined) {
          delete values[name];
        } else {
          values[name] = previous;
        }
        return { values };
      });
      throw new Error(result.error);
    }
  },

  setCells: async (name, edits) => {
    const previous = get().values[name];
    if (!previous || !("Array" in previous) || !previous.Array) {
      throw new Error(`no array value loaded for ${name}`);
    }
    const next = [...previous.Array];
    for (const e of edits) {
      next[e.index] = e.value;
    }
    // Optimistic update; the backend stays the source of truth via tune_dirty.
    set((s) => ({ values: { ...s.values, [name]: { Array: next } as Value } }));
    const result = await commands.setCells(
      name,
      edits.map((e) => ({ index: e.index, value: e.value })),
    );
    if (result.status === "error") {
      set((s) => ({ values: { ...s.values, [name]: previous } }));
      throw new Error(result.error);
    }
  },

  reset: () => set({ ...INITIAL }),
}));
