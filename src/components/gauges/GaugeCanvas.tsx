// SPDX-License-Identifier: GPL-3.0-or-later
//
// Hand-rolled canvas gauges — deliberately NO chart/gauge dependency:
// ARCHITECTURE §3 mandates custom canvas rendering, and the ≤30 Hz realtime
// frames must bypass React reconciliation entirely (the MIT `canvas-gauges`
// library served as a visual reference only, no code was taken).
import { useEffect, useRef } from "react";
import { useRealtimeStore } from "../../stores/realtime";

/** Zone/text colors resolved from the CSS design tokens at mount. */
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
 * the value actually changed since the last painted frame.
 */
export function GaugeCanvas({
  channel,
  width,
  height,
  draw,
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
    const theme = resolveTheme(canvas);

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
        draw(ctx, value, { width, height }, theme);
      }
      frame = requestAnimationFrame(paint);
    };
    frame = requestAnimationFrame(paint);
    return () => cancelAnimationFrame(frame);
  }, [channel, width, height, draw]);

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
