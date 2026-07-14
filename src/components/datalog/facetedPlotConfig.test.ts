// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from "vitest";
import { buildFacetedPlot, type FacetedSeries } from "./facetedPlotConfig";

const seriesA: FacetedSeries = {
  label: "A: RPM",
  x: [0, 1, 2],
  y: [1000, 1500, 2000],
  color: "#2374e1",
};

const seriesB: FacetedSeries = {
  label: "B: RPM",
  x: [0, 1, 2],
  y: [900, 1400, 1900],
  color: "#e15554",
};

const openBounds = { min: null, max: null };

describe("buildFacetedPlot", () => {
  it("prepends a placeholder series ({}) and data column (null) at index 0", () => {
    const { options, data } = buildFacetedPlot(
      [seriesA],
      openBounds,
      openBounds,
      false,
      800,
    );

    expect(options.series?.[0]).toEqual({});
    expect(data[0]).toBeNull();
  });

  it("places a single input series at index 1 with facets scaled to x/y", () => {
    const { options } = buildFacetedPlot(
      [seriesA],
      openBounds,
      openBounds,
      false,
      800,
    );

    expect(options.series?.length).toBe(2);
    expect(options.series?.[1].facets?.[0].scale).toBe("x");
    expect(options.series?.[1].facets?.[1].scale).toBe("y");
  });

  it("places two input series at indices 1 and 2 with labels/colors intact", () => {
    const { options, data } = buildFacetedPlot(
      [seriesA, seriesB],
      openBounds,
      openBounds,
      false,
      800,
    );

    expect(options.series?.length).toBe(3);
    expect(options.series?.[1].label).toBe(seriesA.label);
    expect(options.series?.[1].stroke).toBe(seriesA.color);
    expect(options.series?.[2].label).toBe(seriesB.label);
    expect(options.series?.[2].stroke).toBe(seriesB.color);
    expect(data[1]).toEqual([seriesA.x, seriesA.y]);
    expect(data[2]).toEqual([seriesB.x, seriesB.y]);
  });

  it("configures line series (paths/points/sorted) when scatter is false", () => {
    const { options } = buildFacetedPlot(
      [seriesA],
      openBounds,
      openBounds,
      false,
      800,
    );

    const realSeries = options.series?.[1];
    expect(realSeries?.width).toBe(1);
    expect(realSeries?.points?.show).toBe(false);
    expect(realSeries?.paths).toBeUndefined();
    expect(realSeries?.sorted).toBe(1);
  });

  it("configures scatter series (paths/points/sorted) when scatter is true", () => {
    const { options } = buildFacetedPlot(
      [seriesA],
      openBounds,
      openBounds,
      true,
      800,
    );

    const realSeries = options.series?.[1];
    expect(realSeries?.width).toBe(0);
    expect(realSeries?.points?.show).toBe(true);
    expect(realSeries?.points?.size).toBe(3);
    expect(typeof realSeries?.paths).toBe("function");
    expect(realSeries?.sorted).toBe(0);
  });

  it("floors width at 320 and fixes height at 320", () => {
    const { options } = buildFacetedPlot(
      [seriesA],
      openBounds,
      openBounds,
      false,
      100,
    );

    expect(options.width).toBe(320);
    expect(options.height).toBe(320);
  });

  it("uses the requested width when it exceeds the floor", () => {
    const { options } = buildFacetedPlot(
      [seriesA],
      openBounds,
      openBounds,
      false,
      1024,
    );

    expect(options.width).toBe(1024);
  });

  it("overrides scale range with explicit bounds and falls back to auto range otherwise", () => {
    const { options } = buildFacetedPlot(
      [seriesA],
      { min: 5, max: null },
      { min: null, max: 42 },
      false,
      800,
    );

    const xRange = options.scales?.x.range;
    const yRange = options.scales?.y.range;
    expect(typeof xRange).toBe("function");
    expect(typeof yRange).toBe("function");
    if (typeof xRange === "function") {
      expect(xRange(null as never, 0, 10, "x")).toEqual([5, 10]);
    }
    if (typeof yRange === "function") {
      expect(yRange(null as never, 0, 10, "y")).toEqual([0, 42]);
    }
  });

  it("keeps axes, legend and cursor drag configuration", () => {
    const { options } = buildFacetedPlot(
      [seriesA],
      openBounds,
      openBounds,
      false,
      800,
    );

    expect(options.mode).toBe(2);
    expect(options.axes).toEqual([{ label: "X" }, { label: "Y" }]);
    expect(options.legend).toEqual({ show: true });
    expect(options.cursor?.drag).toEqual({ x: true, y: true, setScale: true });
  });
});
