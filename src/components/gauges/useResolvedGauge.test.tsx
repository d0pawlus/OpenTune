// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, it, expect, beforeEach } from "vitest";
import { renderHook } from "@testing-library/react";
import { useResolvedGauge } from "./useResolvedGauge";
import { useTuneStore } from "../../stores/tune";
import type { GaugeDto } from "../../ipc/bindings";

const gauge: GaugeDto = {
  name: "rpmGauge",
  channel: "rpm",
  title: "RPM",
  units: "RPM",
  low: 0,
  high: 7000,
  lo_danger: null,
  lo_warn: null,
  hi_warn: null,
  hi_danger: null,
  value_digits: 0,
  label_digits: 0,
  category: "",
};

const bound = {
  name: "rpmGauge",
  low: 0,
  high: 8000,
  lo_danger: 300,
  lo_warn: 600,
  hi_warn: 6500,
  hi_danger: 7200,
};

// M2/M3 re-review finding 4: the resolved gauge is a dependency of every
// gauge's `draw` callback, which in turn keys the GaugeCanvas rAF effect —
// an identity-unstable result restarts the paint loop of every mounted
// gauge on every store refresh.
describe("useResolvedGauge identity stability", () => {
  beforeEach(() => {
    useTuneStore.getState().reset();
  });

  it("returns the same object across re-renders while bounds are unchanged", () => {
    useTuneStore.getState().setGaugeBounds([bound]);

    const { result, rerender } = renderHook(() => useResolvedGauge(gauge));
    const first = result.current;
    rerender();

    expect(result.current).toBe(first);
  });

  it("keeps identity across a refresh that resolves identical bounds", () => {
    useTuneStore.getState().setGaugeBounds([bound]);
    const { result, rerender } = renderHook(() => useResolvedGauge(gauge));
    const first = result.current;

    // An edit/undo/burn triggers a re-resolve; the values did not change.
    useTuneStore.getState().setGaugeBounds([{ ...bound }]);
    rerender();

    expect(result.current).toBe(first);
  });

  it("re-resolves when a bound value actually changed", () => {
    useTuneStore.getState().setGaugeBounds([bound]);
    const { result, rerender } = renderHook(() => useResolvedGauge(gauge));
    const first = result.current;

    useTuneStore.getState().setGaugeBounds([{ ...bound, high: 9000 }]);
    rerender();

    expect(result.current).not.toBe(first);
    expect(result.current.high).toBe(9000);
  });

  it("returns the definition gauge unchanged when no bounds are resolved", () => {
    const { result } = renderHook(() => useResolvedGauge(gauge));
    expect(result.current).toBe(gauge);
  });
});
