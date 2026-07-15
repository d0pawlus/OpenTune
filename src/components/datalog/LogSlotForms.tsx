// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import type { LogFormatDto } from "../../ipc/bindings";
import { t, type Locale } from "../../i18n";
import { useDatalogStore, type LogSlot } from "../../stores/datalog";

const formats: { value: LogFormatDto; label: string }[] = [
  { value: "Csv", label: "CSV" },
  { value: "MlgV1", label: "MLG v1" },
];

const LOG_DIALOG_FILTERS = [{ name: "Log", extensions: ["csv", "mlg"] }];

async function pickOpenLogPath(): Promise<string | null> {
  const picked = await open({ multiple: false, filters: LOG_DIALOG_FILTERS });
  return typeof picked === "string" ? picked : null;
}

async function pickSaveLogPath(): Promise<string | null> {
  const picked = await save({ filters: LOG_DIALOG_FILTERS });
  return typeof picked === "string" ? picked : null;
}

export function LogPathForm({
  slot,
  locale,
}: {
  slot: LogSlot;
  locale: Locale;
}) {
  const openLog = useDatalogStore((state) => state.openLog);
  const dataset = useDatalogStore((state) => state.logs[slot]);
  const loading = useDatalogStore((state) => state.loading);
  const [path, setPath] = useState("");
  const [format, setFormat] = useState<LogFormatDto>("Csv");
  return (
    <fieldset className="dl-fieldset">
      <legend>
        {t("datalog.log", locale)} {slot}
      </legend>
      <label>
        {t("datalog.path", locale)}
        <input
          type="text"
          value={path}
          onChange={(event) => setPath(event.target.value)}
          placeholder="/path/to/log.csv"
        />
      </label>
      <button
        type="button"
        disabled={loading}
        onClick={() => {
          void pickOpenLogPath().then((picked) => {
            if (picked) setPath(picked);
          });
        }}
      >
        {t("datalog.browse", locale)}
      </button>
      <label>
        {t("datalog.format", locale)}
        <select
          value={format}
          onChange={(event) => setFormat(event.target.value as LogFormatDto)}
        >
          {formats.map((item) => (
            <option key={item.value} value={item.value}>
              {item.label}
            </option>
          ))}
        </select>
      </label>
      <button
        type="button"
        onClick={() => void openLog(slot, path, format)}
        disabled={!path.trim() || loading}
      >
        {t("datalog.open", locale)}
      </button>
      {dataset && (
        <output>
          {dataset.summary.record_count.toLocaleString()}{" "}
          {t("datalog.rows", locale)}
        </output>
      )}
    </fieldset>
  );
}

export function RecordingControls({ locale }: { locale: Locale }) {
  const recording = useDatalogStore((state) => state.recording);
  const start = useDatalogStore((state) => state.startRecording);
  const stop = useDatalogStore((state) => state.stopRecording);
  const addMarker = useDatalogStore((state) => state.addMarker);
  const refresh = useDatalogStore((state) => state.refreshStatus);
  const [path, setPath] = useState("");
  const [format, setFormat] = useState<LogFormatDto>("Csv");
  const [marker, setMarker] = useState("");

  useEffect(() => {
    void refresh();
  }, [refresh]);
  useEffect(() => {
    if (!recording?.active) return;
    const timer = window.setInterval(() => void refresh(), 1000);
    return () => window.clearInterval(timer);
  }, [recording?.active, refresh]);

  return (
    <fieldset className="dl-fieldset dl-recording">
      <legend>{t("datalog.recording", locale)}</legend>
      <label>
        {t("datalog.path", locale)}
        <input
          type="text"
          value={path}
          onChange={(event) => setPath(event.target.value)}
          placeholder="/path/to/recording.csv"
        />
      </label>
      <button
        type="button"
        disabled={recording?.active}
        onClick={() => {
          void pickSaveLogPath().then((picked) => {
            if (picked) setPath(picked);
          });
        }}
      >
        {t("datalog.browse", locale)}
      </button>
      <label>
        {t("datalog.format", locale)}
        <select
          value={format}
          onChange={(event) => setFormat(event.target.value as LogFormatDto)}
        >
          {formats.map((item) => (
            <option key={item.value} value={item.value}>
              {item.label}
            </option>
          ))}
        </select>
      </label>
      <button
        type="button"
        onClick={() => void start(path, format)}
        disabled={!path.trim() || recording?.active}
      >
        {t("datalog.start", locale)}
      </button>
      <button
        type="button"
        onClick={() => void stop()}
        disabled={!recording?.active}
      >
        {t("datalog.stop", locale)}
      </button>
      <output aria-live="polite">
        {recording?.active
          ? `${recording.record_count.toLocaleString()} ${t("datalog.rows", locale)}`
          : t("datalog.idle", locale)}
      </output>
      <label>
        {t("datalog.marker", locale)}
        <input
          type="text"
          value={marker}
          onChange={(event) => setMarker(event.target.value)}
        />
      </label>
      <button
        type="button"
        disabled={!recording?.active || !marker.trim()}
        onClick={() => {
          void addMarker(marker);
          setMarker("");
        }}
      >
        {t("datalog.addMarker", locale)}
      </button>
    </fieldset>
  );
}

export function ExportControls({ locale }: { locale: Locale }) {
  const exportLog = useDatalogStore((state) => state.exportLog);
  const logs = useDatalogStore((state) => state.logs);
  const [slot, setSlot] = useState<LogSlot>("A");
  const [path, setPath] = useState("");
  const [format, setFormat] = useState<LogFormatDto>("Csv");
  return (
    <fieldset className="dl-fieldset">
      <legend>{t("datalog.export", locale)}</legend>
      <label>
        {t("datalog.log", locale)}
        <select
          value={slot}
          onChange={(event) => setSlot(event.target.value as LogSlot)}
        >
          <option>A</option>
          <option>B</option>
        </select>
      </label>
      <label>
        {t("datalog.path", locale)}
        <input value={path} onChange={(event) => setPath(event.target.value)} />
      </label>
      <button
        type="button"
        onClick={() => {
          void pickSaveLogPath().then((picked) => {
            if (picked) setPath(picked);
          });
        }}
      >
        {t("datalog.browse", locale)}
      </button>
      <label>
        {t("datalog.format", locale)}
        <select
          value={format}
          onChange={(event) => setFormat(event.target.value as LogFormatDto)}
        >
          {formats.map((item) => (
            <option key={item.value} value={item.value}>
              {item.label}
            </option>
          ))}
        </select>
      </label>
      <button
        type="button"
        disabled={!logs[slot] || !path.trim()}
        onClick={() => void exportLog(slot, path, format)}
      >
        {t("datalog.export", locale)}
      </button>
    </fieldset>
  );
}
