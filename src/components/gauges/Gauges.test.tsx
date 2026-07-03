// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import { RoundGauge } from "./RoundGauge";
import { BarGauge } from "./BarGauge";
import { DigitalGauge } from "./DigitalGauge";
import { IndicatorLamp } from "./IndicatorLamp";
import { useRealtimeStore } from "../../stores/realtime";
import type { GaugeDto, IndicatorDto } from "../../ipc/bindings";

const rpmGauge: GaugeDto = {
  name: "rpmGauge",
  channel: "rpm",
  title: "Engine Speed",
  units: "RPM",
  low: 0,
  high: 8000,
  lo_danger: 300,
  lo_warn: 600,
  hi_warn: 6000,
  hi_danger: 7000,
  value_digits: 0,
  label_digits: 0,
  category: "Engine",
};

const indicator: IndicatorDto = {
  expr: "running",
  off_label: "Not running",
  on_label: "Running",
  off_bg: "black",
  off_fg: "white",
  on_bg: "green",
  on_fg: "black",
};

/**
 * jsdom has no real 2D context — provide a recording fake so the imperative
 * rAF paint loop can be observed (any method becomes a vi.fn(), any property
 * assignment is accepted).
 */
function fakeCtx() {
  const record = new Map<string, ReturnType<typeof vi.fn>>();
  const fn = (name: string) => {
    const existing = record.get(name);
    if (existing) return existing;
    const created = vi.fn();
    record.set(name, created);
    return created;
  };
  const target: Record<string | symbol, unknown> = {};
  const ctx = new Proxy(target, {
    get(t, prop) {
      if (typeof prop === "string" && !(prop in t)) {
        t[prop] = fn(prop);
      }
      return t[prop];
    },
    set(t, prop, value) {
      t[prop] = value;
      return true;
    },
  });
  return { ctx: ctx as unknown as CanvasRenderingContext2D, fn };
}

const paintedTexts = (fn: (name: string) => ReturnType<typeof vi.fn>) =>
  fn("fillText").mock.calls.map((call) => call[0]);

describe("canvas gauges", () => {
  let fake: ReturnType<typeof fakeCtx>;

  beforeEach(() => {
    useRealtimeStore.setState({ channels: {} });
    fake = fakeCtx();
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(
      fake.ctx as unknown as ReturnType<HTMLCanvasElement["getContext"]>,
    );
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("RoundGauge paints its title and a neutral — before any frame arrives", async () => {
    render(<RoundGauge gauge={rpmGauge} />);
    expect(screen.getByRole("img", { name: "Engine Speed" })).toBeTruthy();
    await waitFor(() => {
      expect(paintedTexts(fake.fn)).toContain("Engine Speed");
      expect(paintedTexts(fake.fn)).toContain("—");
    });
  });

  it("RoundGauge repaints with the live channel value read from the store", async () => {
    render(<RoundGauge gauge={rpmGauge} />);
    useRealtimeStore.getState().applyFrame({ channels: [["rpm", 3500]] });
    await waitFor(() => {
      expect(paintedTexts(fake.fn)).toContain("3500");
    });
  });

  it("BarGauge paints title, value and units", async () => {
    render(<BarGauge gauge={rpmGauge} />);
    useRealtimeStore.getState().applyFrame({ channels: [["rpm", 4200]] });
    expect(screen.getByRole("img", { name: "Engine Speed" })).toBeTruthy();
    await waitFor(() => {
      expect(paintedTexts(fake.fn)).toContain("Engine Speed");
      expect(paintedTexts(fake.fn)).toContain("4200 RPM");
    });
  });

  it("DigitalGauge paints the formatted readout", async () => {
    render(<DigitalGauge gauge={rpmGauge} />);
    useRealtimeStore.getState().applyFrame({ channels: [["rpm", 900]] });
    await waitFor(() => {
      expect(paintedTexts(fake.fn)).toContain("900");
    });
  });

  it("IndicatorLamp shows the off label until its bit channel is non-zero", async () => {
    render(<IndicatorLamp indicator={indicator} />);
    await waitFor(() => {
      expect(paintedTexts(fake.fn)).toContain("Not running");
    });
    useRealtimeStore.getState().applyFrame({ channels: [["running", 1]] });
    await waitFor(() => {
      expect(paintedTexts(fake.fn)).toContain("Running");
    });
  });

  it("IndicatorLamp fails open to the off state for a comparison expression", async () => {
    render(<IndicatorLamp indicator={{ ...indicator, expr: "rpm > 3000" }} />);
    useRealtimeStore.getState().applyFrame({ channels: [["rpm", 9000]] });
    await waitFor(() => {
      expect(paintedTexts(fake.fn)).toContain("Not running");
    });
    expect(paintedTexts(fake.fn)).not.toContain("Running");
  });

  it("renders inert (no crash) when no 2D context is available", () => {
    vi.mocked(HTMLCanvasElement.prototype.getContext).mockReturnValue(null);
    render(<RoundGauge gauge={rpmGauge} />);
    expect(screen.getByRole("img", { name: "Engine Speed" })).toBeTruthy();
  });
});
