// SPDX-License-Identifier: GPL-3.0-or-later
import { useMemo } from "react";
import type { GaugeDto } from "../../ipc/bindings";
import { useTuneStore } from "../../stores/tune";

/**
 * Overlay backend-resolved tune-dependent bounds onto the immutable
 * definition DTO. This subscription changes only after tune edits/loads;
 * realtime frame updates still bypass React entirely.
 *
 * The result is identity-stable while `gauge` and its bounds are: gauge
 * `draw` callbacks depend on it, and a fresh object per render would
 * restart the GaugeCanvas rAF paint loop.
 */
export function useResolvedGauge(gauge: GaugeDto): GaugeDto {
  const bounds = useTuneStore((state) => state.gaugeBounds[gauge.name]);
  return useMemo(
    () => (bounds ? { ...gauge, ...bounds } : gauge),
    [gauge, bounds],
  );
}
