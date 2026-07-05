// SPDX-License-Identifier: GPL-3.0-or-later
//
// Hand-rolled canvas gauges — deliberately NO chart/gauge dependency:
// ARCHITECTURE §3 mandates custom canvas rendering, and the ≤30 Hz realtime
// frames must bypass React reconciliation entirely (the MIT `canvas-gauges`
// library served as a visual reference only, no code was taken).
import { useEffect, useRef } from "react";
import { useRealtimeStore } from "../../stores/realtime";

/** The app-shell theme name; switching it re-resolves the CSS tokens below. */
export type Theme = "default" | "high-contrast";

/**
 * Zone/text colors resolved from the CSS design tokens. Re-resolved whenever
 * the `theme` prop changes (see {@link GaugeCanvas}), not just at mount.
 */
export interface GaugeTheme {
  ok: string;
  warn: string;
  danger: string;
  text: string;
  surface: string;
  muted: string;
}

/** Imperative painter: receives the latest channel value (or `undefined`). */
export type GaugeDraw = (
  ctx: CanvasRenderingContext2D,
  value: number | undefined,
  size: { width: number; height: number },
  theme: GaugeTheme,
) => void;

interface GaugeCanvasProps {
  /** Output-channel name read from the realtime store ("" = never bound). */
  channel: string;
  width: number;
  height: number;
  draw: GaugeDraw;
  /** Current app-shell theme; a change forces a fresh token resolution. */
  theme: Theme;
  /** Accessible name for the rendered gauge. */
  label: string;
}

function themeToken(
  styles: CSSStyleDeclaration,
  name: string,
  fallback: string,
): string {
  return styles.getPropertyValue(name).trim() || fallback;
}

function resolveTheme(el: HTMLElement): GaugeTheme {
  const styles = getComputedStyle(el);
  return {
    ok: themeToken(styles, "--color-ok", "#2e7d32"),
    warn: themeToken(styles, "--color-warn", "#f9a825"),
    danger: themeToken(styles, "--color-danger", "#c62828"),
    text: themeToken(styles, "--color-text", "#222222"),
    surface: themeToken(styles, "--color-surface", "#ffffff"),
    muted: themeToken(styles, "--color-gauge-muted", "#9e9e9e"),
  };
}

/**
 * Mounts a `<canvas>` and drives a `requestAnimationFrame` paint loop that
 * reads the bound channel **imperatively** from the realtime store
 * (`getState().getChannel(...)`, no selector subscription) — so ≤30 Hz
 * frames never enter React reconciliation. The canvas repaints only when
 * the value actually changed since the last painted frame, *or* when the
 * `theme` prop changes: a theme switch re-resolves the CSS tokens and
 * resets the "already painted" guard so the very next frame repaints even
 * if the bound value is unchanged.
 */
export function GaugeCanvas({
  channel,
  width,
  height,
  draw,
  theme,
  label,
}: GaugeCanvasProps) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    // Fail open: without a 2D context (unsupported environment) the gauge
    // renders as an inert element — it never crashes the panel.
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
    // Re-resolved every time this effect (re-)runs, including on a `theme`
    // prop change — never captured once and reused across theme switches.
    const resolvedTheme = resolveTheme(canvas);

    let frame = 0;
    let painted = false;
    let last: number | undefined;
    const paint = () => {
      const value = useRealtimeStore.getState().getChannel(channel);
      if (!painted || value !== last) {
        painted = true;
        last = value;
        ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
        ctx.clearRect(0, 0, width, height);
        draw(ctx, value, { width, height }, resolvedTheme);
      }
      frame = requestAnimationFrame(paint);
    };
    frame = requestAnimationFrame(paint);
    return () => cancelAnimationFrame(frame);
    // `theme` itself is never read here — it exists purely to invalidate the
    // effect (and reset `painted`/`resolvedTheme`) on a theme switch; the
    // actual colors always come from the live CSS custom properties.
  }, [channel, width, height, draw, theme]);

  return (
    <canvas
      ref={canvasRef}
      className="gauge-canvas"
      style={{ width, height }}
      role="img"
      aria-label={label}
    />
  );
}
