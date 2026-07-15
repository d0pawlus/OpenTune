// SPDX-License-Identifier: GPL-3.0-or-later

export function rowAtTime(
  tMs: readonly (number | null)[],
  targetMs: number,
): number {
  if (tMs.length === 0) return 0;
  let low = 0;
  let high = tMs.length - 1;
  while (low <= high) {
    const middle = (low + high) >>> 1;
    const value = tMs[middle];
    if (value === null || value > targetMs) high = middle - 1;
    else low = middle + 1;
  }
  return Math.max(0, Math.min(tMs.length - 1, high));
}

/**
 * The last recorded (non-null) timestamp in the log, scanned backwards with
 * no array copy. H4: the previous `[...tMs].reverse().find(...)` recomputed
 * a full copy + reverse of up to 100k elements on every animation frame.
 */
export function lastValidTime(tMs: readonly (number | null)[]): number {
  for (let index = tMs.length - 1; index >= 0; index -= 1) {
    const value = tMs[index];
    if (value !== null) return value;
  }
  return 0;
}

export function playbackTarget(
  startLogMs: number,
  elapsedWallMs: number,
  speed: number,
): number {
  return startLogMs + Math.max(0, elapsedWallMs) * Math.max(0, speed);
}
