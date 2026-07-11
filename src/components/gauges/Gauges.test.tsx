// SPDX-License-Identifier: GPL-3.0-or-later
import { useLayoutEffect, useState } from "react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { RoundGauge } from "./RoundGauge";
import { BarGauge } from "./BarGauge";
import { DigitalGauge } from "./DigitalGauge";
import { IndicatorLamp } from "./IndicatorLamp";
import type { Theme } from "./GaugeCanvas";
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
 * `vi.fn()` with no explicit type argument resolves (via `ReturnType`) to an
 * ambiguous `Mock<Procedure | Constructable>` that TS refuses to call
 * directly. Pin the recorder to a single, plainly-callable signature.
 */
type Recorder = ReturnType<typeof vi.fn<(...args: unknown[]) => void>>;

/**
 * jsdom has no real 2D context — provide a recording fake so the imperative
 * rAF paint loop can be observed (any method becomes a vi.fn(), any property
 * assignment is accepted). Property *assignments* are also recorded, under a
 * `set:<prop>` key, so a test can inspect the full history of colors a draw
 * callback assigned (e.g. `fillStyle`) across repaints, not just the latest.
 */
function fakeCtx() {
  const record = new Map<string, Recorder>();
  const fn = (name: string): Recorder => {
    const existing = record.get(name);
    if (existing) return existing;
    const created: Recorder = vi.fn();
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
      if (typeof prop === "string") fn(`set:${prop}`)(value);
      return true;
    },
  });
  return { ctx: ctx as unknown as CanvasRenderingContext2D, fn };
}

const paintedTexts = (fn: (name: string) => Recorder) =>
  fn("fillText").mock.calls.map((call) => call[0]);

const assignedValues = (fn: (name: string) => Recorder, prop: string) =>
  fn(`set:${prop}`).mock.calls.map((call) => call[0]);

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
    render(<RoundGauge gauge={rpmGauge} theme="default" />);
    expect(screen.getByRole("img", { name: "Engine Speed" })).toBeTruthy();
    await waitFor(() => {
      expect(paintedTexts(fake.fn)).toContain("Engine Speed");
      expect(paintedTexts(fake.fn)).toContain("—");
    });
  });

  it("RoundGauge repaints with the live channel value read from the store", async () => {
    render(<RoundGauge gauge={rpmGauge} theme="default" />);
    useRealtimeStore.getState().applyFrame({ channels: [["rpm", 3500]] });
    await waitFor(() => {
      expect(paintedTexts(fake.fn)).toContain("3500");
    });
  });

  it("BarGauge paints title, value and units", async () => {
    render(<BarGauge gauge={rpmGauge} theme="default" />);
    useRealtimeStore.getState().applyFrame({ channels: [["rpm", 4200]] });
    expect(screen.getByRole("img", { name: "Engine Speed" })).toBeTruthy();
    await waitFor(() => {
      expect(paintedTexts(fake.fn)).toContain("Engine Speed");
      expect(paintedTexts(fake.fn)).toContain("4200 RPM");
    });
  });

  it("DigitalGauge paints the formatted readout", async () => {
    render(<DigitalGauge gauge={rpmGauge} theme="default" />);
    useRealtimeStore.getState().applyFrame({ channels: [["rpm", 900]] });
    await waitFor(() => {
      expect(paintedTexts(fake.fn)).toContain("900");
    });
  });

  it("IndicatorLamp shows the off label until its bit channel is non-zero", async () => {
    render(<IndicatorLamp indicator={indicator} theme="default" />);
    await waitFor(() => {
      expect(paintedTexts(fake.fn)).toContain("Not running");
    });
    useRealtimeStore.getState().applyFrame({ channels: [["running", 1]] });
    await waitFor(() => {
      expect(paintedTexts(fake.fn)).toContain("Running");
    });
  });

  it("IndicatorLamp fails open to the off state for a comparison expression", async () => {
    render(
      <IndicatorLamp
        indicator={{ ...indicator, expr: "rpm > 3000" }}
        theme="default"
      />,
    );
    useRealtimeStore.getState().applyFrame({ channels: [["rpm", 9000]] });
    await waitFor(() => {
      expect(paintedTexts(fake.fn)).toContain("Not running");
    });
    expect(paintedTexts(fake.fn)).not.toContain("Running");
  });

  it("renders inert (no crash) when no 2D context is available", () => {
    vi.mocked(HTMLCanvasElement.prototype.getContext).mockReturnValue(null);
    render(<RoundGauge gauge={rpmGauge} theme="default" />);
    expect(screen.getByRole("img", { name: "Engine Speed" })).toBeTruthy();
  });
});

/**
 * An indicator with no INI-supplied colors, so its lamp paints the raw
 * theme palette (`theme.ok` / `theme.muted`) verbatim — nothing overrides it.
 */
const neutralIndicator: IndicatorDto = {
  expr: "running",
  off_label: "Not running",
  on_label: "Running",
  off_bg: "",
  off_fg: "",
  on_bg: "",
  on_fg: "",
};

/**
 * Fakes `getComputedStyle` the way the app's real CSS cascade behaves: the
 * resolved token depends on `document.documentElement.dataset.theme`, read
 * live at call time (never on a value captured earlier or on the component's
 * `theme` prop) — this is what makes the test a genuine ordering check
 * rather than one that trivially passes off the prop alone.
 */
function fakeComputedStyle(): CSSStyleDeclaration {
  const highContrast =
    document.documentElement.dataset.theme === "high-contrast";
  const tokens: Record<string, string> = {
    "--color-ok": highContrast ? "hc-ok" : "default-ok",
    "--color-gauge-muted": highContrast ? "hc-muted" : "default-muted",
  };
  return {
    getPropertyValue: (name: string) => tokens[name] ?? "",
  } as CSSStyleDeclaration;
}

/**
 * Mirrors App.tsx's real theme wiring: local `theme` state, a toggle button,
 * and a *layout* effect that reflects it onto
 * `document.documentElement.dataset.theme` — then threads `theme` down to a
 * real gauge exactly as App → Dashboard → IndicatorLamp does. It must be a
 * layout effect, not a passive one: React flushes all layout effects before
 * any passive effect, so this guarantees the attribute lands before
 * GaugeCanvas's passive effect re-resolves colors off it (child-before-parent
 * passive-effect ordering would otherwise read the outgoing theme).
 */
function ThemeHarness() {
  const [theme, setTheme] = useState<Theme>("default");
  useLayoutEffect(() => {
    document.documentElement.dataset.theme =
      theme === "high-contrast" ? "high-contrast" : "";
  }, [theme]);
  return (
    <div>
      <button
        onClick={() =>
          setTheme((prev) =>
            prev === "high-contrast" ? "default" : "high-contrast",
          )
        }
      >
        toggle theme
      </button>
      <IndicatorLamp indicator={neutralIndicator} theme={theme} />
    </div>
  );
}

describe("GaugeCanvas theme reactivity", () => {
  let fake: ReturnType<typeof fakeCtx>;

  beforeEach(() => {
    useRealtimeStore.setState({ channels: {} });
    document.documentElement.dataset.theme = "";
    fake = fakeCtx();
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(
      fake.ctx as unknown as ReturnType<HTMLCanvasElement["getContext"]>,
    );
    vi.spyOn(window, "getComputedStyle").mockImplementation(fakeComputedStyle);
  });

  afterEach(() => {
    vi.restoreAllMocks();
    document.documentElement.dataset.theme = "";
  });

  it("re-resolves theme colors and repaints on a theme switch, with no channel value change", async () => {
    render(<ThemeHarness />);
    await waitFor(() => {
      expect(assignedValues(fake.fn, "fillStyle")).toContain("default-muted");
    });
    const canvas = screen.getByRole("img", {
      name: "Running",
    }) as HTMLCanvasElement;
    let backingDimensionAssignments = 0;
    for (const property of ["width", "height"] as const) {
      const value = canvas[property];
      Object.defineProperty(canvas, property, {
        configurable: true,
        get: () => value,
        set: () => {
          backingDimensionAssignments += 1;
        },
      });
    }

    fireEvent.click(screen.getByRole("button", { name: "toggle theme" }));

    // The theme effect paints immediately and does not clear the backing
    // bitmap by assigning its already-correct dimensions.
    expect(assignedValues(fake.fn, "fillStyle")).toContain("hc-muted");
    expect(backingDimensionAssignments).toBe(0);
    await waitFor(() => {
      expect(assignedValues(fake.fn, "fillStyle")).toContain("hc-muted");
    });
    // No realtime frame ever arrived — the "off" branch stays selected
    // throughout, so this repaint is attributable to the theme switch alone.
    expect(assignedValues(fake.fn, "fillStyle")).not.toContain("hc-ok");
  });
});
