// SPDX-License-Identifier: GPL-3.0-or-later
// Heatmap coloring for table/curve cell values: a blue (low) to red (high)
// scale on a fixed hue sweep, exposed both as a CSS hsl() string (DOM-side
// table rendering) and as normalized RGB (three.js vertex colors for the 3D
// surface view). Pure functions only.

/** Normalizes `value` into [0, 1] over [lo, hi], clamped at both ends. */
export function heatT(value: number, lo: number, hi: number): number {
  if (lo >= hi) return 0.5; // degenerate range: no gradient to map onto
  const t = (value - lo) / (hi - lo);
  return Math.min(Math.max(t, 0), 1);
}

const HUE_LOW = 220; // blue
const HUE_HIGH = 0; // red
const SATURATION_PCT = 70;
const LIGHTNESS_PCT = 55;

const hueOf = (value: number, lo: number, hi: number): number => {
  const t = heatT(value, lo, hi);
  return Math.round(HUE_LOW + (HUE_HIGH - HUE_LOW) * t);
};

/** CSS `hsl()` string for `value`, blue at `lo` sweeping to red at `hi`. */
export function heatColor(value: number, lo: number, hi: number): string {
  const hue = hueOf(value, lo, hi);
  return `hsl(${hue} ${SATURATION_PCT}% ${LIGHTNESS_PCT}%)`;
}

/** Converts an HSL color (h in degrees, s/l in [0,1]) to RGB, each in [0,1]. */
function hslToRgb(h: number, s: number, l: number): [number, number, number] {
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const hp = h / 60;
  const x = c * (1 - Math.abs((hp % 2) - 1));
  let r1 = 0;
  let g1 = 0;
  let b1 = 0;
  if (hp < 1) [r1, g1, b1] = [c, x, 0];
  else if (hp < 2) [r1, g1, b1] = [x, c, 0];
  else if (hp < 3) [r1, g1, b1] = [0, c, x];
  else if (hp < 4) [r1, g1, b1] = [0, x, c];
  else if (hp < 5) [r1, g1, b1] = [x, 0, c];
  else [r1, g1, b1] = [c, 0, x];
  const m = l - c / 2;
  return [r1 + m, g1 + m, b1 + m];
}

/** Same blue-to-red scale as `heatColor`, as 0..1 RGB for three.js vertex colors. */
export function heatRgb(
  value: number,
  lo: number,
  hi: number,
): [number, number, number] {
  const hue = hueOf(value, lo, hi);
  return hslToRgb(hue, SATURATION_PCT / 100, LIGHTNESS_PCT / 100);
}
