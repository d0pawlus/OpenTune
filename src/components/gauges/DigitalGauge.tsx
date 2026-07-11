// SPDX-License-Identifier: GPL-3.0-or-later
import { useCallback } from "react";
import type { GaugeDto } from "../../ipc/bindings";
import { GaugeCanvas, type GaugeDraw, type Theme } from "./GaugeCanvas";
import { formatValue, zoneColor } from "./gaugeMath";
import { useResolvedGauge } from "./useResolvedGauge";

const WIDTH = 210;
const HEIGHT = 96;

/** Large numeric readout, tinted by its warn/danger zone. */
export function DigitalGauge({
  gauge,
  theme,
}: {
  gauge: GaugeDto;
  theme: Theme;
}) {
  const resolvedGauge = useResolvedGauge(gauge);
  const draw = useCallback<GaugeDraw>(
    (ctx, value, size, theme) => {
      const cx = size.width / 2;

      ctx.fillStyle = theme.text;
      ctx.textAlign = "center";
      ctx.font = "600 13px system-ui, sans-serif";
      ctx.fillText(resolvedGauge.title, cx, 20);

      // Zone-tinted readout ("—" neutral when the channel is unknown).
      ctx.fillStyle =
        value === undefined
          ? theme.text
          : theme[
              zoneColor(
                value,
                resolvedGauge.lo_danger,
                resolvedGauge.lo_warn,
                resolvedGauge.hi_warn,
                resolvedGauge.hi_danger,
              )
            ];
      ctx.font = "700 32px ui-monospace, monospace";
      ctx.fillText(formatValue(value, resolvedGauge.value_digits), cx, 58);

      ctx.fillStyle = theme.text;
      ctx.font = "12px system-ui, sans-serif";
      ctx.globalAlpha = 0.7;
      ctx.fillText(resolvedGauge.units, cx, 80);
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
