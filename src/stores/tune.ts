// SPDX-License-Identifier: GPL-3.0-or-later
import { create } from "zustand";
import { commands } from "../ipc/bindings";
import type {
  DefinitionDto,
  ResolvedGaugeBoundsDto,
  TuneDirtyEvent,
  Value,
} from "../ipc/bindings";

interface TuneStore {
  /** The definition being rendered, or `null` before it is loaded. */
  definition: DefinitionDto | null;
  /** Current physical values keyed by constant name. */
  values: Record<string, Value>;
  /** Tune-resolved gauge bounds keyed by gauge name. */
  gaugeBounds: Record<string, ResolvedGaugeBoundsDto>;
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

  setDefinition: (definition: DefinitionDto | null) => void;
  setValues: (values: Record<string, Value>) => void;
  setGaugeBounds: (bounds: ResolvedGaugeBoundsDto[]) => void;
  setActiveDialog: (activeDialog: string | null) => void;

  /** Reflect a backend `tune_dirty` event. */
  applyDirty: (event: TuneDirtyEvent) => void;

  /**
   * Optimistically set a value and write it live via the backend. The local
   * value updates immediately; if the command fails, it rolls back to the
   * previous value and rethrows so the caller can surface the error. The dirty
   * badge is driven separately by the `tune_dirty` event, not here.
   */
  setValue: (name: string, value: Value) => Promise<void>;

  reset: () => void;
}

const INITIAL: Pick<
  TuneStore,
  | "definition"
  | "values"
  | "gaugeBounds"
  | "dirty"
  | "dirtyPages"
  | "activeDialog"
> = {
  definition: null,
  values: {},
  gaugeBounds: {},
  dirty: false,
  dirtyPages: [],
  activeDialog: null,
};

export const useTuneStore = create<TuneStore>((set, get) => ({
  ...INITIAL,

  setDefinition: (definition) => set({ definition }),
  setValues: (values) => set({ values }),
  setGaugeBounds: (bounds) =>
    set({
      gaugeBounds: Object.fromEntries(
        bounds.map((bound) => [bound.name, bound]),
      ),
    }),
  setActiveDialog: (activeDialog) => set({ activeDialog }),

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

  reset: () => set({ ...INITIAL }),
}));
