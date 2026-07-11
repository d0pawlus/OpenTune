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
  fields: LogFieldDto[];
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
  const fields = [...dataset.summary.fields];
  for (const spec of specs) {
    const source = columns[spec.source];
    if (!source) continue;
    columns[spec.name] = evaluateMathChannel(spec, source, dataset.tMs);
    fields.push({ name: spec.name, units: "" });
  }
  return { ...dataset, fields, columns };
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
    set({ loading: true, error: null, playing: false });
    try {
      const opened = await commands.openLog(path.trim(), format);
      if (opened.status === "error") throw new Error(opened.error);
      const summary = opened.data;
      const tMs: (number | null)[] = [];
      const rawColumns = summary.fields.map(() => [] as (number | null)[]);
      const markers = new Map<string, MarkerDto>();
      let offset = 0;
      while (offset < summary.record_count) {
        const page = await commands.getLogData(offset, PAGE_SIZE);
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
          fields: summary.fields,
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
      const opened = await commands.openLog(source.path, source.format);
      if (opened.status === "error") {
        set({ loading: false, error: opened.error });
        return;
      }
      const saved = await commands.saveLog(path.trim(), format);
      set({
        loading: false,
        activeSlot: slot,
        error: saved.status === "error" ? saved.error : null,
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
    const channels = dataset.fields.map(
      (field) =>
        [field.name, dataset.columns[field.name]?.[row] ?? null] as [
          string,
          number | null,
        ],
    );
    useRealtimeStore.getState().applyFrame({ channels });
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
