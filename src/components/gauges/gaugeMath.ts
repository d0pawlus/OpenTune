// SPDX-License-Identifier: GPL-3.0-or-later
// Pure gauge render math — tested first, consumed by the canvas drawing.

/** Zone classification for a gauge value. */
export type Zone = "ok" | "warn" | "danger";

/** Round gauges sweep 270° starting at 135° (7 o'clock, clockwise). */
export const GAUGE_START_ANGLE = 0.75 * Math.PI;
export const GAUGE_SWEEP = 1.5 * Math.PI;

export interface GaugeGeometry {
  /** 0..1 fill fraction of the gauge range, clamped. */
  fraction: number;
  /** Needle angle in radians along the round-gauge sweep. */
  angle: number;
}

/**
 * Map a value in `[low, high]` to gauge geometry. Degenerate ranges
 * (`high <= low`) and non-finite values degrade to fraction 0 (fail-open —
 * the gauge draws its neutral floor, never crashes).
 */
export function gaugeGeometry(
  value: number,
  low: number,
  high: number,
): GaugeGeometry {
  const usable =
    Number.isFinite(value) &&
    Number.isFinite(low) &&
    Number.isFinite(high) &&
    high > low;
  const fraction = usable
    ? Math.min(1, Math.max(0, (value - low) / (high - low)))
    : 0;
  return { fraction, angle: GAUGE_START_ANGLE + fraction * GAUGE_SWEEP };
}

/**
 * Classify a value against the gauge's warn/danger thresholds. `null`
 * thresholds (INI `{ expr }` bounds project to `null`) never trigger.
 * Boundary semantics: a value *at* a threshold is in that zone, and danger
 * wins over warn.
 */
export function zoneColor(
  value: number,
  loDanger: number | null,
  loWarn: number | null,
  hiWarn: number | null,
  hiDanger: number | null,
): Zone {
  if (loDanger !== null && value <= loDanger) return "danger";
  if (hiDanger !== null && value >= hiDanger) return "danger";
  if (loWarn !== null && value <= loWarn) return "warn";
  if (hiWarn !== null && value >= hiWarn) return "warn";
  return "ok";
}

/** Digits are clamped to a sane display range. */
const MAX_DISPLAY_DIGITS = 6;

/**
 * Fixed-digit readout; unknown (`undefined`) or non-finite values render an
 * em dash — the neutral "no data" state, never a fake 0.
 */
export function formatValue(value: number | undefined, digits: number): string {
  if (value === undefined || !Number.isFinite(value)) return "—";
  return value.toFixed(Math.max(0, Math.min(MAX_DISPLAY_DIGITS, digits)));
}
