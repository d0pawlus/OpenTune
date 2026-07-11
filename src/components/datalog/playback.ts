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

export function playbackTarget(
  startLogMs: number,
  elapsedWallMs: number,
  speed: number,
): number {
  return startLogMs + Math.max(0, elapsedWallMs) * Math.max(0, speed);
}
