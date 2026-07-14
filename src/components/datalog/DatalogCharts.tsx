// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useMemo, useRef } from "react";
import uPlot from "uplot";
import "uplot/dist/uPlot.min.css";
import type { VirtualDynoReportDto } from "../../ipc/bindings";
import type { LogDataset } from "../../stores/datalog";
import {
  buildFacetedPlot,
  type AxisBounds,
  type FacetedSeries,
} from "./facetedPlotConfig";

interface DatalogChartsProps {
  logA?: LogDataset;
  logB?: LogDataset;
  timeChannels: string[];
  scatterX: string;
  scatterY: string;
  xBounds: AxisBounds;
  yBounds: AxisBounds;
}

const palette = ["#2374e1", "#e15554", "#3a9d5d", "#8c5bd6", "#e58b22"];

const MIN_PLOT_WIDTH = 320;
const PLOT_HEIGHT = 320;

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
    const { options, data } = buildFacetedPlot(
      series,
      { min: xBounds.min, max: xBounds.max },
      { min: yBounds.min, max: yBounds.max },
      scatter,
      element.clientWidth || 800,
    );
    const plot = new uPlot(options, data, element);
    const resize = () =>
      plot.setSize({
        width: Math.max(MIN_PLOT_WIDTH, element.clientWidth || 800),
        height: PLOT_HEIGHT,
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
