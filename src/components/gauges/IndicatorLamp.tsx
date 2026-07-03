// SPDX-License-Identifier: GPL-3.0-or-later
import { useCallback } from "react";
import type { IndicatorDto } from "../../ipc/bindings";
import { GaugeCanvas, type GaugeDraw, type Theme } from "./GaugeCanvas";

const WIDTH = 150;
const HEIGHT = 44;

/** A bare output-channel name (the common Speeduino indicator binding). */
const BARE_CHANNEL = /^[A-Za-z_][A-Za-z0-9_]*$/;

/**
 * Boolean indicator lamp. The indicator's `expr` is a bare bit-channel name
 * in the common case — it is looked up in the realtime frame map and any
 * non-zero value lights the lamp. A full comparison expression cannot be
 * evaluated frontend-side, so it fails open to the off state (never crashes
 * the panel).
 */
export function IndicatorLamp({
  indicator,
  theme,
}: {
  indicator: IndicatorDto;
  theme: Theme;
}) {
  const channel = BARE_CHANNEL.test(indicator.expr) ? indicator.expr : "";

  const draw = useCallback<GaugeDraw>(
    (ctx, value, size, theme) => {
      const on = value !== undefined && value !== 0;
      // INI colors are named colors, verbatim. Set a theme fallback first:
      // the canvas ignores invalid `fillStyle` assignments, so an unknown
      // color name degrades to the fallback instead of stale paint state.
      ctx.fillStyle = on ? theme.ok : theme.muted;
      const bg = on ? indicator.on_bg : indicator.off_bg;
      if (bg) ctx.fillStyle = bg;
      ctx.fillRect(0, 0, size.width, size.height);

      ctx.fillStyle = on ? theme.surface : theme.text;
      const fg = on ? indicator.on_fg : indicator.off_fg;
      if (fg) ctx.fillStyle = fg;
      ctx.textAlign = "center";
      ctx.font = "600 13px system-ui, sans-serif";
      ctx.fillText(
        on ? indicator.on_label : indicator.off_label,
        size.width / 2,
        size.height / 2 + 4,
      );
    },
    [indicator],
  );

  return (
    <GaugeCanvas
      channel={channel}
      width={WIDTH}
      height={HEIGHT}
      draw={draw}
      theme={theme}
      label={indicator.on_label || indicator.expr}
    />
  );
}
