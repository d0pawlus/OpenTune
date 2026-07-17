// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useMemo, useRef } from "react";
import uPlot from "uplot";
import "uplot/dist/uPlot.min.css";
import type { VirtualDynoReportDto } from "../../ipc/bindings";
import type { LogDataset } from "../../stores/datalog";
import { t, type Locale } from "../../i18n";
import {
  buildFacetedPlot,
  MIN_PLOT_WIDTH,
  PLOT_HEIGHT,
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
  locale: Locale;
}

const palette = ["#2374e1", "#e15554", "#3a9d5d", "#8c5bd6", "#e58b22"];

function useFacetedPlot(
  series: FacetedSeries[],
  xBounds: AxisBounds,
  yBounds: AxisBounds,
  scatter: boolean,
  xLabel: string,
  yLabel: string,
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
      xLabel,
      yLabel,
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
  }, [
    scatter,
    series,
    xBounds.max,
    xBounds.min,
    yBounds.max,
    yBounds.min,
    xLabel,
    yLabel,
  ]);
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
  xLabel,
  yLabel,
  noDataText,
}: {
  series: FacetedSeries[];
  xBounds: AxisBounds;
  yBounds: AxisBounds;
  scatter: boolean;
  label: string;
  xLabel: string;
  yLabel: string;
  noDataText: string;
}) {
  const host = useFacetedPlot(
    series,
    xBounds,
    yBounds,
    scatter,
    xLabel,
    yLabel,
  );
  return (
    <div className="dl-chart" role="img" aria-label={label}>
      {series.length === 0 ? <p>{noDataText}</p> : <div ref={host} />}
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
  locale,
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

  const xAxisLabel = t("datalog.xAxis", locale);
  const yAxisLabel = t("datalog.yAxis", locale);
  const noChartData = t("datalog.noChartData", locale);

  return (
    <div className="dl-chart-grid">
      <FacetedPlot
        label={t("datalog.timeSeriesPlot", locale)}
        series={timeSeries}
        xBounds={xBounds}
        yBounds={yBounds}
        scatter={false}
        xLabel={xAxisLabel}
        yLabel={yAxisLabel}
        noDataText={noChartData}
      />
      <FacetedPlot
        label={t("datalog.scatterPlot", locale)}
        series={scatterSeries}
        xBounds={xBounds}
        yBounds={yBounds}
        scatter
        xLabel={xAxisLabel}
        yLabel={yAxisLabel}
        noDataText={noChartData}
      />
    </div>
  );
}

export function DynoChart({
  report,
  locale,
}: {
  report: VirtualDynoReportDto;
  locale: Locale;
}) {
  const series = useMemo(() => {
    const points = [...report.points].sort(
      (a, b) => (a.rpm ?? 0) - (b.rpm ?? 0),
    );
    return [
      {
        label: t("datalog.whp", locale),
        x: points.map((point) => point.rpm),
        y: points.map((point) => point.wheel_hp),
        color: palette[0],
      },
      {
        label: t("datalog.torqueNm", locale),
        x: points.map((point) => point.rpm),
        y: points.map((point) => point.estimated_engine_torque_nm),
        color: palette[1],
      },
    ];
  }, [report, locale]);
  return (
    <FacetedPlot
      label={t("datalog.dynoChartLabel", locale)}
      series={series}
      xBounds={{ min: null, max: null }}
      yBounds={{ min: null, max: null }}
      scatter={false}
      xLabel={t("datalog.xAxis", locale)}
      yLabel={t("datalog.yAxis", locale)}
      noDataText={t("datalog.noChartData", locale)}
    />
  );
}
