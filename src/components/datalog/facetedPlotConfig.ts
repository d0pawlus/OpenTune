// SPDX-License-Identifier: GPL-3.0-or-later
import type uPlot from "uplot";

/** Shared plot dimensions — imported by `DatalogCharts` so the builder and
 * the resize handler can never drift apart. */
export const MIN_PLOT_WIDTH = 320;
export const PLOT_HEIGHT = 320;
const SCATTER_POINT_SIZE = 3;

export interface AxisBounds {
  min: number | null;
  max: number | null;
}

export interface FacetedSeries {
  label: string;
  x: (number | null)[];
  y: (number | null)[];
  color: string;
}

export interface FacetedPlotConfig {
  options: uPlot.Options;
  data: uPlot.AlignedData;
}

/**
 * Builds the options/data pair for a uPlot `mode: 2` (faceted) chart.
 *
 * uPlot's faceted mode reserves index 0 of both `series` and `data` for a
 * placeholder: `setDefaults2` force-overwrites `series[0]` with `{}`, the
 * constructor reads `series[1].facets[0].scale` (one series short of the
 * real data throws otherwise), and internal draw loops start at index 1.
 * Real series must therefore start at index 1 in both arrays, with a `{}`
 * series and a `null` data column at index 0.
 */
export function buildFacetedPlot(
  series: FacetedSeries[],
  xBounds: AxisBounds,
  yBounds: AxisBounds,
  scatter: boolean,
  width: number,
  xLabel = "X",
  yLabel = "Y",
): FacetedPlotConfig {
  const realSeries: uPlot.Series[] = series.map((item) => ({
    label: item.label,
    stroke: item.color,
    width: scatter ? 0 : 1,
    points: { show: scatter, size: scatter ? SCATTER_POINT_SIZE : 0 },
    paths: scatter ? () => null : undefined,
    sorted: scatter ? (0 as const) : (1 as const),
    facets: [
      { scale: "x", auto: true },
      { scale: "y", auto: true },
    ],
  }));

  const options: uPlot.Options = {
    mode: 2,
    width: Math.max(MIN_PLOT_WIDTH, width),
    height: PLOT_HEIGHT,
    series: [{}, ...realSeries],
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
    axes: [{ label: xLabel }, { label: yLabel }],
    legend: { show: true },
    cursor: { drag: { x: true, y: true, setScale: true } },
  };

  const realData = series.map((item) => [item.x, item.y]);
  const data = [null, ...realData] as unknown as uPlot.AlignedData;

  return { options, data };
}
