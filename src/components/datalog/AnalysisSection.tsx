// SPDX-License-Identifier: GPL-3.0-or-later
import { lazy, Suspense, useMemo, useState } from "react";
import { commands } from "../../ipc/bindings";
import type {
  AnomalyReportDto,
  LogStatsReportDto,
  VirtualDynoReportDto,
} from "../../ipc/bindings";
import { t, type Locale } from "../../i18n";
import { useDatalogStore, type LogSlot } from "../../stores/datalog";
import { ChannelSelect, NumberField } from "./DatalogFormFields";

const LazyDynoChart = lazy(() =>
  import("./DatalogCharts").then((module) => ({
    default: module.DynoChart,
  })),
);

export function Analysis({ locale }: { locale: Locale }) {
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
                <th>{t("datalog.statMin", locale)}</th>
                <th>{t("datalog.statMax", locale)}</th>
                <th>{t("datalog.statMean", locale)}</th>
                <th>{t("datalog.statStdDev", locale)}</th>
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
          label={t("datalog.afr", locale)}
          value={selectedAfr}
          channels={channels}
          setValue={setAfrChannel}
        />
        <ChannelSelect
          label={t("datalog.rpm", locale)}
          value={selectedRpm}
          channels={channels}
          setValue={setRpmChannel}
        />
        <ChannelSelect
          label={t("datalog.load", locale)}
          value={selectedLoad}
          channels={channels}
          setValue={setLoadChannel}
        />
        <ChannelSelect
          label={t("datalog.knock", locale)}
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
            label={t("datalog.rpm", locale)}
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
          <NumberField
            label={t("datalog.dragCoefficient", locale)}
            value={drag}
            min={0}
            onChange={setDrag}
          />
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
            <LazyDynoChart report={dyno} locale={locale} />
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
