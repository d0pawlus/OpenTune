// SPDX-License-Identifier: GPL-3.0-or-later
import { useCallback } from "react";
import type { GaugeDto } from "../../ipc/bindings";
import { GaugeCanvas, type GaugeDraw, type Theme } from "./GaugeCanvas";
import { formatValue, gaugeGeometry, zoneColor } from "./gaugeMath";
import { useResolvedGauge } from "./useResolvedGauge";

const WIDTH = 220;
const HEIGHT = 72;
const PAD = 12;
const BAR_TOP = 34;
const BAR_HEIGHT = 16;

/** Horizontal fill bar with a zone-colored fill and inline readout. */
export function BarGauge({ gauge, theme }: { gauge: GaugeDto; theme: Theme }) {
  const resolvedGauge = useResolvedGauge(gauge);
  const draw = useCallback<GaugeDraw>(
    (ctx, value, size, theme) => {
      const low = resolvedGauge.low;
      const high = resolvedGauge.high;
      const hasRange =
        low !== null &&
        high !== null &&
        Number.isFinite(low) &&
        Number.isFinite(high) &&
        high > low;
      const range = hasRange
        ? { low: low as number, high: high as number }
        : null;
      const barWidth = size.width - 2 * PAD;

      // Muted track.
      ctx.fillStyle = theme.muted;
      ctx.globalAlpha = 0.3;
      ctx.fillRect(PAD, BAR_TOP, barWidth, BAR_HEIGHT);
      ctx.globalAlpha = 1;

      if (value !== undefined && range) {
        const { fraction } = gaugeGeometry(value, range.low, range.high);
        const zone = zoneColor(
          value,
          resolvedGauge.lo_danger,
          resolvedGauge.lo_warn,
          resolvedGauge.hi_warn,
          resolvedGauge.hi_danger,
        );
        ctx.fillStyle = theme[zone];
        ctx.fillRect(PAD, BAR_TOP, barWidth * fraction, BAR_HEIGHT);
      }

      // Title (left) and readout (right) above the bar.
      ctx.fillStyle = theme.text;
      ctx.textAlign = "left";
      ctx.font = "600 13px system-ui, sans-serif";
      ctx.fillText(resolvedGauge.title, PAD, 22);
      ctx.textAlign = "right";
      ctx.font = "700 14px ui-monospace, monospace";
      const readout = formatValue(value, resolvedGauge.value_digits);
      ctx.fillText(
        resolvedGauge.units ? `${readout} ${resolvedGauge.units}` : readout,
        size.width - PAD,
        22,
      );

      // Scale end labels under the bar.
      ctx.font = "10px system-ui, sans-serif";
      ctx.globalAlpha = 0.7;
      ctx.textAlign = "left";
      ctx.fillText(
        range ? formatValue(range.low, resolvedGauge.label_digits) : "—",
        PAD,
        size.height - 6,
      );
      ctx.textAlign = "right";
      ctx.fillText(
        range ? formatValue(range.high, resolvedGauge.label_digits) : "—",
        size.width - PAD,
        size.height - 6,
      );
      ctx.globalAlpha = 1;
    },
    [resolvedGauge],
  );

  return (
    <GaugeCanvas
      channel={resolvedGauge.channel}
      width={WIDTH}
      height={HEIGHT}
      draw={draw}
      theme={theme}
      label={resolvedGauge.title}
    />
  );
}
