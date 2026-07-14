// SPDX-License-Identifier: GPL-3.0-or-later
import { lazy, Suspense, useEffect, useMemo, useRef, useState } from "react";
import { commands } from "../../ipc/bindings";
import type {
  AnomalyReportDto,
  LogFormatDto,
  LogStatsReportDto,
  VirtualDynoReportDto,
} from "../../ipc/bindings";
import { t, type Locale } from "../../i18n";
import { useDatalogStore, type LogSlot } from "../../stores/datalog";
import type { MathOperation } from "./mathChannels";
import { lastValidTime, playbackTarget, rowAtTime } from "./playback";
import "./datalog.css";

const LazyCharts = lazy(() =>
  import("./DatalogCharts").then((module) => ({
    default: module.DatalogCharts,
  })),
);
const LazyDynoChart = lazy(() =>
  import("./DatalogCharts").then((module) => ({
    default: module.DynoChart,
  })),
);

const formats: { value: LogFormatDto; label: string }[] = [
  { value: "Csv", label: "CSV" },
  { value: "MlgV1", label: "MLG v1" },
];

const NumberField = ({
  label,
  value,
  onChange,
  min,
  max,
  step = "any",
}: {
  label: string;
  value: number;
  onChange: (value: number) => void;
  min?: number;
  max?: number;
  step?: number | "any";
}) => (
  <label>
    {label}
    <input
      type="number"
      value={value}
      min={min}
      max={max}
      step={step}
      onChange={(event) => onChange(Number(event.target.value))}
    />
  </label>
);

const ChannelSelect = ({
  label,
  value,
  channels,
  setValue,
}: {
  label: string;
  value: string;
  channels: string[];
  setValue: (value: string) => void;
}) => (
  <label>
    {label}
    <select value={value} onChange={(event) => setValue(event.target.value)}>
      {channels.map((channel) => (
        <option key={channel}>{channel}</option>
      ))}
    </select>
  </label>
);

function LogPathForm({ slot, locale }: { slot: LogSlot; locale: Locale }) {
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

function RecordingControls({ locale }: { locale: Locale }) {
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

function Playback({ locale }: { locale: Locale }) {
  const dataset = useDatalogStore((state) => state.logs.A ?? state.logs.B);
  const row = useDatalogStore((state) => state.playbackRow);
  const playing = useDatalogStore((state) => state.playing);
  const replaying = useDatalogStore((state) => state.replaying);
  const speed = useDatalogStore((state) => state.speed);
  const setRow = useDatalogStore((state) => state.setPlaybackRow);
  const setPlaying = useDatalogStore((state) => state.setPlaying);
  const setSpeed = useDatalogStore((state) => state.setSpeed);
  const stopPlayback = useDatalogStore((state) => state.stopPlayback);
  const animation = useRef<number | null>(null);
  // H4: the tick loop reads the *current* row from this ref rather than from
  // a `row` effect-dependency, so scrubbing/advancing playback never tears
  // down and restarts the rAF loop (which used to reset the wall-clock time
  // base every single frame and drift the reported log time).
  const rowRef = useRef(row);
  useEffect(() => {
    rowRef.current = row;
  }, [row]);

  // H4: scanned backwards with no array copy, and memoized so it is
  // recomputed only when the dataset itself changes, not every frame.
  const finalTime = useMemo(() => lastValidTime(dataset?.tMs ?? []), [dataset]);

  useEffect(() => {
    if (!playing || !dataset || dataset.tMs.length === 0) return;
    const startedAt = performance.now();
    const startLog = dataset.tMs[rowRef.current] ?? 0;
    const tick = (now: number) => {
      const target = playbackTarget(startLog, now - startedAt, speed);
      if (target >= finalTime) {
        stopPlayback();
        return;
      }
      setRow(rowAtTime(dataset.tMs, target));
      animation.current = requestAnimationFrame(tick);
    };
    animation.current = requestAnimationFrame(tick);
    return () => {
      if (animation.current !== null) cancelAnimationFrame(animation.current);
    };
  }, [dataset, playing, speed, finalTime, setRow, stopPlayback]);

  useEffect(
    () => () => {
      useDatalogStore.getState().stopPlayback();
    },
    [],
  );

  const max = Math.max(0, (dataset?.summary.record_count ?? 1) - 1);
  return (
    <fieldset className="dl-fieldset dl-playback">
      <legend>{t("datalog.playback", locale)}</legend>
      <button
        type="button"
        disabled={!dataset}
        aria-pressed={playing}
        onClick={() => setPlaying(!playing)}
      >
        {playing ? t("datalog.pause", locale) : t("datalog.play", locale)}
      </button>
      <button type="button" disabled={!replaying} onClick={stopPlayback}>
        {t("datalog.stopPlayback", locale)}
      </button>
      <label>
        {t("datalog.position", locale)}
        <input
          type="range"
          min={0}
          max={max}
          value={Math.min(row, max)}
          disabled={!dataset}
          onChange={(event) => setRow(Number(event.target.value))}
          onKeyDown={(event) => {
            if (event.key === "Home") setRow(0);
            if (event.key === "End") setRow(max);
          }}
        />
      </label>
      <output>
        {row.toLocaleString()} / {max.toLocaleString()}
      </output>
      <label>
        {t("datalog.speed", locale)}
        <select
          value={speed}
          onChange={(event) => setSpeed(Number(event.target.value))}
        >
          {[0.25, 0.5, 1, 2, 4, 8].map((value) => (
            <option key={value} value={value}>
              {value}×
            </option>
          ))}
        </select>
      </label>
      {replaying && (
        <p role="status" className="dl-replay-indicator">
          {t("datalog.replaying", locale)}
        </p>
      )}
    </fieldset>
  );
}

function MathChannelLibrary({ locale }: { locale: Locale }) {
  const logs = useDatalogStore((state) => state.logs);
  const specs = useDatalogStore((state) => state.mathChannels);
  const add = useDatalogStore((state) => state.addMathChannel);
  const remove = useDatalogStore((state) => state.removeMathChannel);
  const channels = useMemo(
    () => (logs.A ?? logs.B)?.summary.fields.map((field) => field.name) ?? [],
    [logs],
  );
  const [source, setSource] = useState("");
  const [name, setName] = useState("");
  const [kind, setKind] = useState<MathOperation["kind"]>("derivative");
  const [first, setFirst] = useState(5);
  const [second, setSecond] = useState(100);
  const selectedSource = source || channels[0] || "";

  const create = () => {
    if (!selectedSource || !name.trim()) return;
    let operation: MathOperation;
    if (kind === "movingAverage") operation = { kind, window: first };
    else if (kind === "lowPass") operation = { kind, alpha: first };
    else if (kind === "gate") operation = { kind, min: first, max: second };
    else operation = { kind };
    add({
      id: `${Date.now()}-${name.trim()}`,
      name: name.trim(),
      source: selectedSource,
      operation,
    });
    setName("");
  };

  return (
    <fieldset className="dl-fieldset">
      <legend>{t("datalog.math", locale)}</legend>
      <label>
        {t("datalog.source", locale)}
        <select
          value={selectedSource}
          onChange={(event) => setSource(event.target.value)}
        >
          {channels.map((channel) => (
            <option key={channel}>{channel}</option>
          ))}
        </select>
      </label>
      <label>
        {t("datalog.operation", locale)}
        <select
          value={kind}
          onChange={(event) =>
            setKind(event.target.value as MathOperation["kind"])
          }
        >
          <option value="derivative">{t("datalog.derivative", locale)}</option>
          <option value="movingAverage">
            {t("datalog.movingAverage", locale)}
          </option>
          <option value="lowPass">{t("datalog.lowPass", locale)}</option>
          <option value="gate">{t("datalog.gate", locale)}</option>
        </select>
      </label>
      {kind === "movingAverage" && (
        <NumberField
          label={t("datalog.window", locale)}
          value={first}
          min={1}
          step={1}
          onChange={setFirst}
        />
      )}
      {kind === "lowPass" && (
        <NumberField
          label={t("datalog.alpha", locale)}
          value={first}
          min={0}
          max={1}
          step={0.05}
          onChange={setFirst}
        />
      )}
      {kind === "gate" && (
        <>
          <NumberField
            label={t("datalog.minimum", locale)}
            value={first}
            onChange={setFirst}
          />
          <NumberField
            label={t("datalog.maximum", locale)}
            value={second}
            onChange={setSecond}
          />
        </>
      )}
      <label>
        {t("datalog.name", locale)}
        <input value={name} onChange={(event) => setName(event.target.value)} />
      </label>
      <button
        type="button"
        onClick={create}
        disabled={!selectedSource || !name.trim()}
      >
        {t("datalog.create", locale)}
      </button>
      <ul className="dl-inline-list">
        {specs.map((spec) => (
          <li key={spec.id}>
            {spec.name} ← {spec.source}
            <button type="button" onClick={() => remove(spec.id)}>
              {t("datalog.remove", locale)}
            </button>
          </li>
        ))}
      </ul>
    </fieldset>
  );
}

function ExportControls({ locale }: { locale: Locale }) {
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

function Analysis({ locale }: { locale: Locale }) {
  const logs = useDatalogStore((state) => state.logs);
  const activeSlot = useDatalogStore((state) => state.activeSlot);
  const openLog = useDatalogStore((state) => state.openLog);
  const [slot, setSlot] = useState<LogSlot>("A");
  const dataset = logs[slot];
  const channels = useMemo(
    () => dataset?.fields.map((field) => field.name) ?? [],
    [dataset],
  );
  const [stats, setStats] = useState<LogStatsReportDto | null>(null);
  const [anomalies, setAnomalies] = useState<AnomalyReportDto | null>(null);
  const [dyno, setDyno] = useState<VirtualDynoReportDto | null>(null);
  const [speedChannel, setSpeedChannel] = useState("");
  const [rpmChannel, setRpmChannel] = useState("");
  const [afrChannel, setAfrChannel] = useState("");
  const [loadChannel, setLoadChannel] = useState("");
  const [knockChannel, setKnockChannel] = useState("");
  const [sensorChannel, setSensorChannel] = useState("");
  const [sensorMin, setSensorMin] = useState(0);
  const [sensorMax, setSensorMax] = useState(100);
  const [mass, setMass] = useState(1500);
  const [drag, setDrag] = useState(0.32);
  const [area, setArea] = useState(2.2);
  const [rolling, setRolling] = useState(0.012);
  const [loss, setLoss] = useState(0.15);
  const [airDensity, setAirDensity] = useState(1.225);
  const [windowSize, setWindowSize] = useState(5);
  const [leanAfr, setLeanAfr] = useState(15);
  const [knockThreshold, setKnockThreshold] = useState(1);
  const [busy, setBusy] = useState(false);
  const [analysisError, setAnalysisError] = useState<string | null>(null);
  const fallback = channels[0] ?? "";
  const selectedSpeed = speedChannel || fallback;
  const selectedRpm =
    rpmChannel || channels.find((name) => /rpm/i.test(name)) || fallback;
  const selectedAfr =
    afrChannel || channels.find((name) => /afr/i.test(name)) || fallback;
  const selectedLoad =
    loadChannel || channels.find((name) => /load|map/i.test(name)) || fallback;
  const selectedKnock =
    knockChannel || channels.find((name) => /knock/i.test(name)) || fallback;
  const selectedSensor = sensorChannel || fallback;

  // Re-open the dataset's log into the backend for analysis if it isn't
  // already the current `opened_log`, then hand back the log_id the
  // analysis commands must send — the freshly re-opened id when a reopen
  // happened, or the slot's already-current id otherwise. Returns null when
  // there is no dataset, or the reopen didn't make this slot active.
  const activate = async (): Promise<number | null> => {
    if (!dataset) return null;
    if (activeSlot !== slot) await openLog(slot, dataset.path, dataset.format);
    const state = useDatalogStore.getState();
    if (state.activeSlot !== slot) return null;
    return state.logs[slot]?.logId ?? null;
  };
  const runStats = async () => {
    setBusy(true);
    setAnalysisError(null);
    try {
      const logId = await activate();
      if (logId !== null) {
        const result = await commands.logStats(logId, {
          channels,
          reject_when: [],
        });
        if (result.status === "ok") setStats(result.data);
        else setAnalysisError(result.error);
      }
    } catch (error) {
      setAnalysisError(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };
  const runAnomalies = async () => {
    setBusy(true);
    setAnalysisError(null);
    try {
      const logId = await activate();
      if (logId !== null) {
        const result = await commands.detectAnomaly(logId, {
          sensors: selectedSensor
            ? [{ channel: selectedSensor, min: sensorMin, max: sensorMax }]
            : [],
          afr_channel: selectedAfr,
          lean_afr: leanAfr,
          lean_min_rpm: 0,
          rpm_channel: selectedRpm,
          load_channel: selectedLoad,
          lean_min_load: 0,
          knock_channel: selectedKnock,
          knock_threshold: knockThreshold,
          knock_min_rpm: 0,
        });
        if (result.status === "ok") setAnomalies(result.data);
        else setAnalysisError(result.error);
      }
    } catch (error) {
      setAnalysisError(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };
  const runDyno = async () => {
    setBusy(true);
    setAnalysisError(null);
    try {
      const logId = await activate();
      if (logId !== null) {
        const result = await commands.virtualDyno(logId, {
          speed_channel: selectedSpeed,
          rpm_channel: selectedRpm,
          mass_kg: mass,
          drag_coefficient: drag,
          frontal_area_m2: area,
          rolling_resistance: rolling,
          drivetrain_loss: loss,
          smoothing_window: Math.max(1, Math.round(windowSize)),
          air_density_kg_m3: airDensity,
        });
        if (result.status === "ok") setDyno(result.data);
        else setAnalysisError(result.error);
      }
    } catch (error) {
      setAnalysisError(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="dl-analysis">
      <h3>{t("datalog.analysis", locale)}</h3>
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
      <div className="dl-analysis-actions">
        <button
          type="button"
          disabled={!dataset || busy}
          onClick={() => void runStats()}
        >
          {t("datalog.stats", locale)}
        </button>
        <button
          type="button"
          disabled={!dataset || busy}
          onClick={() => void runAnomalies()}
        >
          {t("datalog.anomalies", locale)}
        </button>
      </div>
      {analysisError && (
        <p role="alert" className="dl-error">
          {analysisError}
        </p>
      )}
      {stats && (
        <div className="dl-table-wrap">
          <table>
            <caption>
              {t("datalog.stats", locale)}: {stats.accepted_rows}/
              {stats.total_rows}
            </caption>
            <thead>
              <tr>
                <th>{t("datalog.channel", locale)}</th>
                <th>Min</th>
                <th>Max</th>
                <th>Mean</th>
                <th>σ</th>
              </tr>
            </thead>
            <tbody>
              {stats.stats.map((item) => (
                <tr key={item.channel}>
                  <th>{item.channel}</th>
                  <td>{item.min?.toFixed(2) ?? "—"}</td>
                  <td>{item.max?.toFixed(2) ?? "—"}</td>
                  <td>{item.mean?.toFixed(2) ?? "—"}</td>
                  <td>{item.std_dev?.toFixed(2) ?? "—"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <div className="dl-form-grid">
        <ChannelSelect
          label="AFR"
          value={selectedAfr}
          channels={channels}
          setValue={setAfrChannel}
        />
        <ChannelSelect
          label="RPM"
          value={selectedRpm}
          channels={channels}
          setValue={setRpmChannel}
        />
        <ChannelSelect
          label="Load"
          value={selectedLoad}
          channels={channels}
          setValue={setLoadChannel}
        />
        <ChannelSelect
          label="Knock"
          value={selectedKnock}
          channels={channels}
          setValue={setKnockChannel}
        />
        <ChannelSelect
          label={t("datalog.sensorRange", locale)}
          value={selectedSensor}
          channels={channels}
          setValue={setSensorChannel}
        />
        <NumberField
          label={t("datalog.leanAfr", locale)}
          value={leanAfr}
          onChange={setLeanAfr}
        />
        <NumberField
          label={t("datalog.knockThreshold", locale)}
          value={knockThreshold}
          onChange={setKnockThreshold}
        />
        <NumberField
          label={t("datalog.sensorMin", locale)}
          value={sensorMin}
          onChange={setSensorMin}
        />
        <NumberField
          label={t("datalog.sensorMax", locale)}
          value={sensorMax}
          onChange={setSensorMax}
        />
      </div>
      {anomalies && (
        <div>
          <h4>
            {t("datalog.findings", locale)} ({anomalies.anomalies.length})
          </h4>
          <ul className="dl-findings">
            {anomalies.anomalies.map((item, index) => (
              <li key={`${item.row}-${item.channel}-${index}`}>
                #{item.row} · {item.kind} · {item.channel}: {item.value ?? "—"}{" "}
                ({item.threshold})
              </li>
            ))}
          </ul>
        </div>
      )}
      <fieldset className="dl-fieldset">
        <legend>{t("datalog.dyno", locale)}</legend>
        <div className="dl-form-grid">
          <ChannelSelect
            label={t("datalog.speedChannel", locale)}
            value={selectedSpeed}
            channels={channels}
            setValue={setSpeedChannel}
          />
          <ChannelSelect
            label="RPM"
            value={selectedRpm}
            channels={channels}
            setValue={setRpmChannel}
          />
          <NumberField
            label={t("datalog.mass", locale)}
            value={mass}
            min={1}
            onChange={setMass}
          />
          <NumberField label="Cd" value={drag} min={0} onChange={setDrag} />
          <NumberField
            label={t("datalog.frontalArea", locale)}
            value={area}
            min={0}
            onChange={setArea}
          />
          <NumberField
            label={t("datalog.rolling", locale)}
            value={rolling}
            min={0}
            onChange={setRolling}
          />
          <NumberField
            label={t("datalog.loss", locale)}
            value={loss}
            min={0}
            max={0.99}
            onChange={setLoss}
          />
          <NumberField
            label={t("datalog.airDensity", locale)}
            value={airDensity}
            min={0}
            onChange={setAirDensity}
          />
          <NumberField
            label={t("datalog.window", locale)}
            value={windowSize}
            min={1}
            step={1}
            onChange={setWindowSize}
          />
        </div>
        <button
          type="button"
          disabled={!dataset || busy || !selectedSpeed || !selectedRpm}
          onClick={() => void runDyno()}
        >
          {t("datalog.runDyno", locale)}
        </button>
      </fieldset>
      {dyno && (
        <div>
          <Suspense fallback={<p>{t("datalog.loadingCharts", locale)}</p>}>
            <LazyDynoChart report={dyno} />
          </Suspense>
          <h4>{t("datalog.assumptions", locale)}</h4>
          <ul>
            {dyno.assumptions.map((item) => (
              <li key={item}>{item}</li>
            ))}
          </ul>
          <h4>{t("datalog.conditions", locale)}</h4>
          <p>
            {dyno.conditions.filter((item) => item.accepted).length}/
            {dyno.conditions.length} {t("datalog.rowsAccepted", locale)}
          </p>
          <ul className="dl-findings">
            {dyno.conditions
              .filter((item) => !item.accepted)
              .slice(0, 100)
              .map((item) => (
                <li key={item.row}>
                  #{item.row}: {item.reason}
                </li>
              ))}
          </ul>
        </div>
      )}
    </section>
  );
}

export function DatalogPanel({ locale }: { locale: Locale }) {
  const logs = useDatalogStore((state) => state.logs);
  const loading = useDatalogStore((state) => state.loading);
  const error = useDatalogStore((state) => state.error);
  const [timeChannels, setTimeChannels] = useState<string[]>([]);
  const [scatterX, setScatterX] = useState("$time");
  const [scatterY, setScatterY] = useState("");
  const [xMin, setXMin] = useState("");
  const [xMax, setXMax] = useState("");
  const [yMin, setYMin] = useState("");
  const [yMax, setYMax] = useState("");
  const channels = useMemo(() => {
    const dataset = logs.A ?? logs.B;
    if (!dataset) return [];
    // Chart pickers offer real + derived channels (M5 review H3 kept math
    // names out of `dataset.fields` so analysis commands never see them).
    return [
      ...dataset.fields.map((field) => field.name),
      ...dataset.mathChannelNames,
    ];
  }, [logs]);
  const selectedTimeChannels =
    timeChannels.length > 0 ? timeChannels : channels.slice(0, 1);
  const selectedScatterY = scatterY || channels[0] || "";

  const bound = (value: string) => {
    const parsed = Number(value);
    return value.trim() && Number.isFinite(parsed) ? parsed : null;
  };
  const markers = [
    ...(logs.A?.markers.map((marker) => ({ slot: "A", marker })) ?? []),
    ...(logs.B?.markers.map((marker) => ({ slot: "B", marker })) ?? []),
  ];

  return (
    <section className="datalog" aria-labelledby="datalog-title">
      <header>
        <h2 id="datalog-title">{t("datalog.title", locale)}</h2>
      </header>
      <p className="dl-offline">{t("datalog.offline", locale)}</p>
      {error && (
        <p role="alert" className="dl-error">
          {error}
        </p>
      )}
      {loading && <p role="status">{t("datalog.loading", locale)}</p>}
      <RecordingControls locale={locale} />
      <div className="dl-two-column">
        <LogPathForm slot="A" locale={locale} />
        <LogPathForm slot="B" locale={locale} />
      </div>
      <Playback locale={locale} />
      <MathChannelLibrary locale={locale} />
      <fieldset className="dl-fieldset">
        <legend>{t("datalog.charts", locale)}</legend>
        <label className="dl-channel-picker">
          {t("datalog.timeChannels", locale)}
          <select
            multiple
            size={Math.min(6, Math.max(2, channels.length))}
            value={selectedTimeChannels}
            onChange={(event) =>
              setTimeChannels(
                [...event.target.selectedOptions].map((option) => option.value),
              )
            }
          >
            {channels.map((channel) => (
              <option key={channel}>{channel}</option>
            ))}
          </select>
        </label>
        <label>
          {t("datalog.scatterX", locale)}
          <select
            value={scatterX}
            onChange={(event) => setScatterX(event.target.value)}
          >
            <option value="$time">{t("datalog.time", locale)}</option>
            {channels.map((channel) => (
              <option key={channel}>{channel}</option>
            ))}
          </select>
        </label>
        <label>
          {t("datalog.scatterY", locale)}
          <select
            value={selectedScatterY}
            onChange={(event) => setScatterY(event.target.value)}
          >
            {channels.map((channel) => (
              <option key={channel}>{channel}</option>
            ))}
          </select>
        </label>
        <div className="dl-axis-grid">
          <label>
            X min
            <input
              type="number"
              value={xMin}
              onChange={(event) => setXMin(event.target.value)}
            />
          </label>
          <label>
            X max
            <input
              type="number"
              value={xMax}
              onChange={(event) => setXMax(event.target.value)}
            />
          </label>
          <label>
            Y min
            <input
              type="number"
              value={yMin}
              onChange={(event) => setYMin(event.target.value)}
            />
          </label>
          <label>
            Y max
            <input
              type="number"
              value={yMax}
              onChange={(event) => setYMax(event.target.value)}
            />
          </label>
        </div>
      </fieldset>
      {(logs.A || logs.B) && (
        <Suspense fallback={<p>{t("datalog.loadingCharts", locale)}</p>}>
          <LazyCharts
            logA={logs.A}
            logB={logs.B}
            timeChannels={selectedTimeChannels}
            scatterX={scatterX}
            scatterY={selectedScatterY}
            xBounds={{ min: bound(xMin), max: bound(xMax) }}
            yBounds={{ min: bound(yMin), max: bound(yMax) }}
          />
        </Suspense>
      )}
      <section>
        <h3>{t("datalog.markers", locale)}</h3>
        {markers.length === 0 ? (
          <p>{t("datalog.noMarkers", locale)}</p>
        ) : (
          <ol>
            {markers.map(({ slot, marker }) => (
              <li key={`${slot}-${marker.record_index}-${marker.text}`}>
                {slot} · #{marker.record_index} · {marker.text}
              </li>
            ))}
          </ol>
        )}
      </section>
      <ExportControls locale={locale} />
      <Analysis locale={locale} />
    </section>
  );
}
