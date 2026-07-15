// SPDX-License-Identifier: GPL-3.0-or-later
import { lazy, Suspense, useMemo, useState } from "react";
import { t, type Locale } from "../../i18n";
import { useDatalogStore } from "../../stores/datalog";

const LazyCharts = lazy(() =>
  import("./DatalogCharts").then((module) => ({
    default: module.DatalogCharts,
  })),
);

export function ChartControls({ locale }: { locale: Locale }) {
  const logs = useDatalogStore((state) => state.logs);
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

  return (
    <>
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
            {t("datalog.xMin", locale)}
            <input
              type="number"
              value={xMin}
              onChange={(event) => setXMin(event.target.value)}
            />
          </label>
          <label>
            {t("datalog.xMax", locale)}
            <input
              type="number"
              value={xMax}
              onChange={(event) => setXMax(event.target.value)}
            />
          </label>
          <label>
            {t("datalog.yMin", locale)}
            <input
              type="number"
              value={yMin}
              onChange={(event) => setYMin(event.target.value)}
            />
          </label>
          <label>
            {t("datalog.yMax", locale)}
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
            locale={locale}
          />
        </Suspense>
      )}
    </>
  );
}
