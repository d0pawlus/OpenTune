// SPDX-License-Identifier: GPL-3.0-or-later
import { useCallback } from "react";
import type { GaugeDto } from "../../ipc/bindings";
import { GaugeCanvas, type GaugeDraw, type Theme } from "./GaugeCanvas";
import {
  GAUGE_START_ANGLE,
  GAUGE_SWEEP,
  formatValue,
  gaugeGeometry,
  zoneColor,
} from "./gaugeMath";
import { useResolvedGauge } from "./useResolvedGauge";

const WIDTH = 210;
const HEIGHT = 160;
const TRACK_WIDTH = 12;

/** Classic 270° sweep gauge: zone-colored value arc, needle and readout. */
export function RoundGauge({
  gauge,
  theme,
}: {
  gauge: GaugeDto;
  theme: Theme;
}) {
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
      const cx = size.width / 2;
      const cy = size.height / 2 + 14;
      const radius = Math.min(size.width, size.height) / 2 - 14;

      // Muted full-sweep track.
      ctx.lineWidth = TRACK_WIDTH;
      ctx.lineCap = "round";
      ctx.strokeStyle = theme.muted;
      ctx.globalAlpha = 0.3;
      ctx.beginPath();
      ctx.arc(
        cx,
        cy,
        radius,
        GAUGE_START_ANGLE,
        GAUGE_START_ANGLE + GAUGE_SWEEP,
      );
      ctx.stroke();
      ctx.globalAlpha = 1;

      if (value !== undefined && range) {
        const { angle } = gaugeGeometry(value, range.low, range.high);
        const zone = zoneColor(
          value,
          resolvedGauge.lo_danger,
          resolvedGauge.lo_warn,
          resolvedGauge.hi_warn,
          resolvedGauge.hi_danger,
        );
        // Value arc in the zone color.
        ctx.strokeStyle = theme[zone];
        ctx.beginPath();
        ctx.arc(cx, cy, radius, GAUGE_START_ANGLE, angle);
        ctx.stroke();
        // Needle.
        ctx.strokeStyle = theme.text;
        ctx.lineWidth = 2;
        ctx.beginPath();
        ctx.moveTo(cx, cy);
        ctx.lineTo(
          cx + Math.cos(angle) * (radius - TRACK_WIDTH),
          cy + Math.sin(angle) * (radius - TRACK_WIDTH),
        );
        ctx.stroke();
      }

      // Title, readout, units and scale end labels.
      ctx.fillStyle = theme.text;
      ctx.textAlign = "center";
      ctx.font = "600 13px system-ui, sans-serif";
      ctx.fillText(resolvedGauge.title, cx, 18);
      ctx.font = "700 24px ui-monospace, monospace";
      ctx.fillText(
        formatValue(value, resolvedGauge.value_digits),
        cx,
        cy + 8,
      );
      ctx.font = "12px system-ui, sans-serif";
      ctx.globalAlpha = 0.7;
      ctx.fillText(resolvedGauge.units, cx, cy + 26);
      ctx.font = "10px system-ui, sans-serif";
      ctx.textAlign = "left";
      ctx.fillText(
        range ? formatValue(range.low, resolvedGauge.label_digits) : "—",
        10,
        size.height - 6,
      );
      ctx.textAlign = "right";
      ctx.fillText(
        range ? formatValue(range.high, resolvedGauge.label_digits) : "—",
        size.width - 10,
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
