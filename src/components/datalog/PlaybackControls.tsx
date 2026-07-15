// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useMemo, useRef } from "react";
import { t, type Locale } from "../../i18n";
import { useDatalogStore } from "../../stores/datalog";
import { lastValidTime, playbackTarget, rowAtTime } from "./playback";

export function Playback({ locale }: { locale: Locale }) {
  const dataset = useDatalogStore((state) => state.logs.A ?? state.logs.B);
  const row = useDatalogStore((state) => state.playbackRow);
  const playing = useDatalogStore((state) => state.playing);
  const replaying = useDatalogStore((state) => state.replaying);
  const speed = useDatalogStore((state) => state.speed);
  const setRow = useDatalogStore((state) => state.setPlaybackRow);
  const setPlaying = useDatalogStore((state) => state.setPlaying);
  const setSpeed = useDatalogStore((state) => state.setSpeed);
  const stopPlayback = useDatalogStore((state) => state.stopPlayback);
  const animation = useRef<number | null>(null);
  // H4: the tick loop reads the *current* row from this ref rather than from
  // a `row` effect-dependency, so scrubbing/advancing playback never tears
  // down and restarts the rAF loop (which used to reset the wall-clock time
  // base every single frame and drift the reported log time).
  const rowRef = useRef(row);
  useEffect(() => {
    rowRef.current = row;
  }, [row]);

  // H4: scanned backwards with no array copy, and memoized so it is
  // recomputed only when the dataset itself changes, not every frame.
  const finalTime = useMemo(() => lastValidTime(dataset?.tMs ?? []), [dataset]);

  useEffect(() => {
    if (!playing || !dataset || dataset.tMs.length === 0) return;
    const startedAt = performance.now();
    const startLog = dataset.tMs[rowRef.current] ?? 0;
    const tick = (now: number) => {
      const target = playbackTarget(startLog, now - startedAt, speed);
      if (target >= finalTime) {
        stopPlayback();
        return;
      }
      setRow(rowAtTime(dataset.tMs, target));
      animation.current = requestAnimationFrame(tick);
    };
    animation.current = requestAnimationFrame(tick);
    return () => {
      if (animation.current !== null) cancelAnimationFrame(animation.current);
    };
  }, [dataset, playing, speed, finalTime, setRow, stopPlayback]);

  useEffect(
    () => () => {
      useDatalogStore.getState().stopPlayback();
    },
    [],
  );

  const max = Math.max(0, (dataset?.summary.record_count ?? 1) - 1);
  return (
    <fieldset className="dl-fieldset dl-playback">
      <legend>{t("datalog.playback", locale)}</legend>
      <button
        type="button"
        disabled={!dataset}
        aria-pressed={playing}
        onClick={() => setPlaying(!playing)}
      >
        {playing ? t("datalog.pause", locale) : t("datalog.play", locale)}
      </button>
      <button type="button" disabled={!replaying} onClick={stopPlayback}>
        {t("datalog.stopPlayback", locale)}
      </button>
      <label>
        {t("datalog.position", locale)}
        <input
          type="range"
          min={0}
          max={max}
          value={Math.min(row, max)}
          disabled={!dataset}
          onChange={(event) => setRow(Number(event.target.value))}
          onKeyDown={(event) => {
            if (event.key === "Home") setRow(0);
            if (event.key === "End") setRow(max);
          }}
        />
      </label>
      <output>
        {row.toLocaleString()} / {max.toLocaleString()}
      </output>
      <label>
        {t("datalog.speed", locale)}
        <select
          value={speed}
          onChange={(event) => setSpeed(Number(event.target.value))}
        >
          {[0.25, 0.5, 1, 2, 4, 8].map((value) => (
            <option key={value} value={value}>
              {value}×
            </option>
          ))}
        </select>
      </label>
      {replaying && (
        <p role="status" className="dl-replay-indicator">
          {t("datalog.replaying", locale)}
        </p>
      )}
    </fieldset>
  );
}
