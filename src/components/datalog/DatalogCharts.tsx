// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useMemo, useRef } from "react";
import uPlot from "uplot";
import "uplot/dist/uPlot.min.css";
import type { VirtualDynoReportDto } from "../../ipc/bindings";
import type { LogDataset } from "../../stores/datalog";

interface AxisBounds {
  min: number | null;
  max: number | null;
}

interface DatalogChartsProps {
  logA?: LogDataset;
  logB?: LogDataset;
  timeChannels: string[];
  scatterX: string;
  scatterY: string;
  xBounds: AxisBounds;
  yBounds: AxisBounds;
}

interface FacetedSeries {
  label: string;
  x: (number | null)[];
  y: (number | null)[];
  color: string;
}

const palette = ["#2374e1", "#e15554", "#3a9d5d", "#8c5bd6", "#e58b22"];

function useFacetedPlot(
  series: FacetedSeries[],
  xBounds: AxisBounds,
  yBounds: AxisBounds,
  scatter: boolean,
) {
  const host = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const element = host.current;
    if (!element || series.length === 0) return;
    const plotSeries = series.map((item) => ({
      label: item.label,
      stroke: item.color,
      width: scatter ? 0 : 1,
      points: { show: scatter, size: scatter ? 3 : 0 },
      paths: scatter ? () => null : undefined,
      sorted: scatter ? (0 as const) : (1 as const),
      facets: [
        { scale: "x", auto: true },
        { scale: "y", auto: true },
      ],
    }));
    const options: uPlot.Options = {
      mode: 2,
      width: Math.max(320, element.clientWidth || 800),
      height: 320,
      series: plotSeries,
      scales: {
        x: {
          time: false,
          range: (_plot, minimum, maximum) => [
            xBounds.min ?? minimum,
            xBounds.max ?? maximum,
          ],
        },
        y: {
          range: (_plot, minimum, maximum) => [
            yBounds.min ?? minimum,
            yBounds.max ?? maximum,
          ],
        },
      },
      axes: [{ label: "X" }, { label: "Y" }],
      legend: { show: true },
      cursor: { drag: { x: true, y: true, setScale: true } },
    };
    const data = series.map((item) => [item.x, item.y]);
    const plot = new uPlot(
      options,
      data as unknown as uPlot.AlignedData,
      element,
    );
    const resize = () =>
      plot.setSize({
        width: Math.max(320, element.clientWidth || 800),
        height: 320,
      });
    window.addEventListener("resize", resize);
    return () => {
      window.removeEventListener("resize", resize);
      plot.destroy();
    };
  }, [scatter, series, xBounds.max, xBounds.min, yBounds.max, yBounds.min]);
  return host;
}

const valueColumn = (log: LogDataset, name: string): (number | null)[] =>
  name === "$time"
    ? log.tMs.map((value) => (value === null ? null : value / 1000))
    : (log.columns[name] ?? []);

function FacetedPlot({
  series,
  xBounds,
  yBounds,
  scatter,
  label,
}: {
  series: FacetedSeries[];
  xBounds: AxisBounds;
  yBounds: AxisBounds;
  scatter: boolean;
  label: string;
}) {
  const host = useFacetedPlot(series, xBounds, yBounds, scatter);
  return (
    <div className="dl-chart" role="img" aria-label={label}>
      {series.length === 0 ? <p>No chart data</p> : <div ref={host} />}
    </div>
  );
}

export function DatalogCharts({
  logA,
  logB,
  timeChannels,
  scatterX,
  scatterY,
  xBounds,
  yBounds,
}: DatalogChartsProps) {
  const logs = useMemo(
    () =>
      [
        ["A", logA],
        ["B", logB],
      ] as const,
    [logA, logB],
  );
  const timeSeries = useMemo(
    () =>
      logs.flatMap(([slot, log], logIndex) =>
        log
          ? timeChannels.flatMap((channel, channelIndex) => {
              const y = log.columns[channel];
              if (!y) return [];
              return [
                {
                  label: `${slot}: ${channel}`,
                  x: valueColumn(log, "$time"),
                  y,
                  color:
                    palette[
                      (channelIndex + logIndex * timeChannels.length) %
                        palette.length
                    ],
                },
              ];
            })
          : [],
      ),
    [logs, timeChannels],
  );
  const scatterSeries = useMemo(
    () =>
      logs.flatMap(([slot, log], index) => {
        if (!log) return [];
        const x = valueColumn(log, scatterX);
        const y = log.columns[scatterY];
        return x.length > 0 && y
          ? [{ label: slot, x, y, color: palette[index] }]
          : [];
      }),
    [logs, scatterX, scatterY],
  );

  return (
    <div className="dl-chart-grid">
      <FacetedPlot
        label="Time-series plot"
        series={timeSeries}
        xBounds={xBounds}
        yBounds={yBounds}
        scatter={false}
      />
      <FacetedPlot
        label="Scatter plot"
        series={scatterSeries}
        xBounds={xBounds}
        yBounds={yBounds}
        scatter
      />
    </div>
  );
}

export function DynoChart({ report }: { report: VirtualDynoReportDto }) {
  const series = useMemo(() => {
    const points = [...report.points].sort(
      (a, b) => (a.rpm ?? 0) - (b.rpm ?? 0),
    );
    return [
      {
        label: "WHP",
        x: points.map((point) => point.rpm),
        y: points.map((point) => point.wheel_hp),
        color: palette[0],
      },
      {
        label: "Torque Nm",
        x: points.map((point) => point.rpm),
        y: points.map((point) => point.estimated_engine_torque_nm),
        color: palette[1],
      },
    ];
  }, [report]);
  return (
    <FacetedPlot
      label="Virtual dyno WHP and torque curves"
      series={series}
      xBounds={{ min: null, max: null }}
      yBounds={{ min: null, max: null }}
      scatter={false}
    />
  );
}
