// SPDX-License-Identifier: GPL-3.0-or-later
import { create } from "zustand";
import { commands } from "../ipc/bindings";
import type {
  LogFieldDto,
  LogFormatDto,
  LogStatusDto,
  LogSummaryDto,
  MarkerDto,
} from "../ipc/bindings";
import {
  evaluateMathChannel,
  type MathChannelSpec,
} from "../components/datalog/mathChannels";
import { useRealtimeStore } from "./realtime";

export type LogSlot = "A" | "B";

export interface LogDataset {
  path: string;
  format: LogFormatDto;
  summary: LogSummaryDto;
  /** The generation token this dataset's data was fetched under (M5 review
   * CRITICAL — C2): every later page/save/analysis call for this slot must
   * send it back, so a stale in-flight request can never splice a
   * different log's rows into this dataset. */
  logId: number;
  fields: LogFieldDto[];
  /** Derived (math) channel names, kept out of `fields` (M5 review HIGH —
   * H3): analysis commands (`runStats`, anomaly, dyno) send every name in
   * `fields` straight to the backend, which rejects unknown names with
   * `MissingChannel`. These are still merged into `columns` so charts keep
   * offering them alongside real channels. */
  mathChannelNames: string[];
  tMs: (number | null)[];
  columns: Record<string, (number | null)[]>;
  markers: MarkerDto[];
}

interface DatalogStore {
  logs: Partial<Record<LogSlot, LogDataset>>;
  activeSlot: LogSlot | null;
  recording: LogStatusDto | null;
  loading: boolean;
  error: string | null;
  mathChannels: MathChannelSpec[];
  playbackRow: number;
  playing: boolean;
  replaying: boolean;
  speed: number;
  openLog: (slot: LogSlot, path: string, format: LogFormatDto) => Promise<void>;
  startRecording: (path: string, format: LogFormatDto) => Promise<void>;
  stopRecording: () => Promise<void>;
  refreshStatus: () => Promise<void>;
  addMarker: (text: string) => Promise<void>;
  exportLog: (
    slot: LogSlot,
    path: string,
    format: LogFormatDto,
  ) => Promise<void>;
  addMathChannel: (spec: MathChannelSpec) => void;
  removeMathChannel: (id: string) => void;
  setPlaybackRow: (row: number) => void;
  setPlaying: (playing: boolean) => void;
  setSpeed: (speed: number) => void;
  stopPlayback: () => void;
  clearError: () => void;
  reset: () => void;
}

const PAGE_SIZE = 20_000;

const withMathChannels = (
  dataset: LogDataset,
  specs: MathChannelSpec[],
): LogDataset => {
  const columns = { ...dataset.columns };
  const mathChannelNames: string[] = [];
  for (const spec of specs) {
    const source = columns[spec.source];
    if (!source) continue;
    columns[spec.name] = evaluateMathChannel(spec, source, dataset.tMs);
    mathChannelNames.push(spec.name);
  }
  // `fields` is intentionally left untouched here — it must only ever hold
  // real backend channel names (see the `mathChannelNames` doc comment).
  return { ...dataset, mathChannelNames, columns };
};

const message = (error: unknown): string =>
  error instanceof Error ? error.message : String(error);

const initial = () => ({
  logs: {},
  activeSlot: null,
  recording: null,
  loading: false,
  error: null,
  mathChannels: [],
  playbackRow: 0,
  playing: false,
  replaying: false,
  speed: 1,
});

export const useDatalogStore = create<DatalogStore>((set, get) => ({
  ...initial(),

  openLog: async (slot, path, format) => {
    if (!path.trim()) {
      set({ error: "A log path is required." });
      return;
    }
    // H2: `logs.A` always wins dataset-selector priority over `logs.B` (see
    // the Playback component), so this slot only backs an active replay
    // when its current dataset *is* that prioritized one. Unloading it here
    // — reopening the slot that is being replayed — must not leave the
    // dashboard frozen behind a stale `replaying: true` with no dataset left
    // to drive it. Loading into the *other*, inactive slot (A/B exist for
    // side-by-side comparison) must not disturb the active replay.
    const priorState = get();
    const unloadsActiveReplay =
      priorState.replaying &&
      (priorState.logs.A ?? priorState.logs.B) === priorState.logs[slot];
    if (unloadsActiveReplay) priorState.stopPlayback();
    set({ loading: true, error: null, playing: false });
    try {
      const opened = await commands.openLog(path.trim(), format);
      if (opened.status === "error") throw new Error(opened.error);
      const summary = opened.data;
      const logId = summary.log_id;
      const tMs: (number | null)[] = [];
      const rawColumns = summary.fields.map(() => [] as (number | null)[]);
      const markers = new Map<string, MarkerDto>();
      let offset = 0;
      while (offset < summary.record_count) {
        // A stale `logId` (another open/stop superseded this one while this
        // loop was still paging) comes back as a typed error here — thrown
        // like any other page error, so the loop aborts and no partial
        // dataset is ever committed to the store (see the `catch` below).
        const page = await commands.getLogData(logId, offset, PAGE_SIZE);
        if (page.status === "error") throw new Error(page.error);
        if (page.data.offset !== offset) {
          throw new Error(`Unexpected log page offset ${page.data.offset}.`);
        }
        tMs.push(...page.data.t_ms);
        page.data.columns.forEach((column, index) => {
          rawColumns[index]?.push(...column);
        });
        page.data.markers.forEach((marker) => {
          markers.set(
            `${marker.record_index}:${marker.t_ms}:${marker.text}`,
            marker,
          );
        });
        const count = page.data.t_ms.length;
        if (count === 0) break;
        offset += count;
      }
      const columns = Object.fromEntries(
        summary.fields.map((field, index) => [
          field.name,
          rawColumns[index] ?? [],
        ]),
      );
      const dataset = withMathChannels(
        {
          path: path.trim(),
          format,
          summary,
          logId,
          fields: summary.fields,
          mathChannelNames: [],
          tMs,
          columns,
          markers: [...markers.values()].sort(
            (a, b) => a.record_index - b.record_index,
          ),
        },
        get().mathChannels,
      );
      set((state) => ({
        logs: { ...state.logs, [slot]: dataset },
        activeSlot: slot,
        playbackRow: 0,
        loading: false,
      }));
    } catch (error) {
      set({ error: message(error), loading: false });
    }
  },

  startRecording: async (path, format) => {
    if (!path.trim()) {
      set({ error: "A recording path is required." });
      return;
    }
    set({ error: null });
    try {
      const result = await commands.startLog(path.trim(), format);
      if (result.status === "error") set({ error: result.error });
      else set({ recording: result.data });
    } catch (error) {
      set({ error: message(error) });
    }
  },

  stopRecording: async () => {
    set({ error: null });
    try {
      const result = await commands.stopLog();
      if (result.status === "error") set({ error: result.error });
      else {
        set((state) => ({
          recording: state.recording
            ? { ...state.recording, active: false }
            : null,
          // Final M5-fixes review (Important 1): `stop_log` auto-opens the
          // just-recorded log under a NEW generation token. Clearing
          // `activeSlot` forces the next analysis click down `activate()`'s
          // reopen path instead of resending a now-stale `logId`.
          activeSlot: null,
        }));
      }
    } catch (error) {
      set({ error: message(error) });
    }
  },

  refreshStatus: async () => {
    try {
      const result = await commands.logStatus();
      if (result.status === "ok") set({ recording: result.data });
      else set({ error: result.error });
    } catch (error) {
      set({ error: message(error) });
    }
  },

  addMarker: async (text) => {
    if (!text.trim()) return;
    try {
      const result = await commands.addLogMarker(text.trim());
      if (result.status === "error") set({ error: result.error });
    } catch (error) {
      set({ error: message(error) });
    }
  },

  exportLog: async (slot, path, format) => {
    const source = get().logs[slot];
    if (!source || !path.trim()) return;
    set({ loading: true, error: null });
    try {
      // Re-open the source into the backend to guarantee it is the current
      // `opened_log` before saving — this mints a FRESH log_id, which is
      // what `saveLog` must send (the slot's previously stored id is stale
      // the instant this reopen assigns a new generation).
      const opened = await commands.openLog(source.path, source.format);
      if (opened.status === "error") {
        set({ loading: false, error: opened.error });
        return;
      }
      const logId = opened.data.log_id;
      const saved = await commands.saveLog(logId, path.trim(), format);
      set((state) => {
        const current = state.logs[slot];
        return {
          loading: false,
          activeSlot: slot,
          error: saved.status === "error" ? saved.error : null,
          // Keep the slot's stored id in sync with the backend generation
          // this reopen just assigned (same rule as `activate()`'s reopen).
          logs: current
            ? { ...state.logs, [slot]: { ...current, logId } }
            : state.logs,
        };
      });
    } catch (error) {
      set({ loading: false, error: message(error) });
    }
  },

  addMathChannel: (spec) =>
    set((state) => {
      const mathChannels = [
        ...state.mathChannels.filter((item) => item.name !== spec.name),
        spec,
      ];
      const logs = Object.fromEntries(
        Object.entries(state.logs).map(([slot, dataset]) => [
          slot,
          withMathChannels(dataset, mathChannels),
        ]),
      ) as Partial<Record<LogSlot, LogDataset>>;
      return { mathChannels, logs };
    }),

  removeMathChannel: (id) =>
    set((state) => {
      const mathChannels = state.mathChannels.filter((item) => item.id !== id);
      const logs = Object.fromEntries(
        Object.entries(state.logs).map(([slot, dataset]) => {
          const baseColumns = Object.fromEntries(
            dataset.summary.fields.map((field) => [
              field.name,
              dataset.columns[field.name],
            ]),
          );
          return [
            slot,
            withMathChannels(
              {
                ...dataset,
                fields: dataset.summary.fields,
                columns: baseColumns,
              },
              mathChannels,
            ),
          ];
        }),
      ) as Partial<Record<LogSlot, LogDataset>>;
      return { mathChannels, logs };
    }),

  setPlaybackRow: (requestedRow) => {
    const dataset = get().logs.A ?? get().logs.B;
    if (!dataset) return;
    const row = Math.max(
      0,
      Math.min(dataset.summary.record_count - 1, Math.round(requestedRow)),
    );
    // Replay must drive every gauge the dataset can feed, real fields and
    // derived math channels alike (M5 review H3 kept math names out of
    // `fields`, so they must be added back in explicitly here).
    const channelNames = [
      ...dataset.fields.map((field) => field.name),
      ...dataset.mathChannelNames,
    ];
    const channels = channelNames.map(
      (name) =>
        [name, dataset.columns[name]?.[row] ?? null] as [string, number | null],
    );
    // Replay semantics, not live semantics: a null column entry is a real
    // recorded gap and must clear the channel (see `applyReplayRow`).
    useRealtimeStore.getState().applyReplayRow({ channels });
    set({ playbackRow: row, replaying: true });
  },
  setPlaying: (playing) => {
    if (playing) get().setPlaybackRow(get().playbackRow);
    set({ playing, replaying: true });
  },
  setSpeed: (speed) => set({ speed: Math.min(8, Math.max(0.25, speed)) }),
  stopPlayback: () => {
    useRealtimeStore.getState().clear();
    set({ playing: false, replaying: false, playbackRow: 0 });
  },
  clearError: () => set({ error: null }),
  reset: () => {
    useRealtimeStore.getState().clear();
    set(initial());
  },
}));
