// SPDX-License-Identifier: GPL-3.0-or-later
import type { GaugeDto } from "../../ipc/bindings";
import { useTuneStore } from "../../stores/tune";

/**
 * Overlay backend-resolved tune-dependent bounds onto the immutable
 * definition DTO. This subscription changes only after tune edits/loads;
 * realtime frame updates still bypass React entirely.
 */
export function useResolvedGauge(gauge: GaugeDto): GaugeDto {
  const bounds = useTuneStore((state) => state.gaugeBounds[gauge.name]);
  return bounds ? { ...gauge, ...bounds } : gauge;
}
