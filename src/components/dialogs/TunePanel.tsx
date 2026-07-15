// SPDX-License-Identifier: GPL-3.0-or-later
import { useCallback, useEffect, useRef, useState } from "react";
import { commands, events } from "../../ipc/bindings";
import type { DefinitionDto, Value } from "../../ipc/bindings";
import { isLinkAlive, useConnectionStore } from "../../stores/connection";
import { useTuneStore } from "../../stores/tune";
import { t, type Locale } from "../../i18n";
import { DialogEngine } from "./DialogEngine";
import { resolveMenuTarget } from "./menuTarget";
import { TableEditor } from "../table-editor/TableEditor";
import { CurveEditor } from "../curve-editor/CurveEditor";
import { TuneDiff } from "../diff/TuneDiff";
import "./dialogs.css";

/** Distinct `visible`/`enable` expressions across every dialog. */
function conditionExprs(def: DefinitionDto): string[] {
  const set = new Set<string>();
  for (const dialog of def.dialogs) {
    for (const field of dialog.fields) {
      if (field.visible) set.add(field.visible);
      if (field.enable) set.add(field.enable);
    }
  }
  return [...set];
}

/**
 * Container for the data-driven tune UI: loads the definition + tune when
 * connected, renders the menu → dialog tree, shows the "modified, not burned"
 * badge, and wires burn/undo/redo. Dirty state flows from the backend via the
 * `tune_dirty` event; field values and visibility are re-read from the backend
 * after every edit (single source of truth).
 *
 * `Dashboard` reads the same `useTuneStore`-held `definition`, so this panel's
 * reset-on-disconnect effect must not fire on a mere `reconnecting` glitch —
 * doing so would null `definition` out from under `Dashboard` and unmount it
 * too. The reset effect therefore keys off {@link isLinkAlive} (`connected`
 * or `reconnecting`), not `isConnected`: only a true disconnect
 * (`disconnected`/`failed`) resets the store, and even then only for a
 * live-read tune — a file-backed offline tune (`useTuneStore`'s `offline`
 * flag, set via `OfflinePanel`'s `setOfflineDefinition`) has no wire link to
 * lose and survives so editing can continue after unplugging. The render
 * gate itself only checks `definition`, so an offline tune still renders
 * with no link at all; `Dashboard` keeps gating on `isLinkAlive` too, so it
 * still hides (no stale gauges) whenever the link is down, offline or not.
 * Wire-touching actions stay connected-only: the load/refresh sequence
 * fetches from the ECU only when nothing is loaded yet and the link
 * *becomes* alive (i.e. on connect — `reconnecting` only ever follows
 * `connected`, so it never re-fires mid-glitch), and burn is disabled
 * while merely reconnecting (`!dirty || !isConnected`). Undo/redo are
 * wire-free for an offline tune (Task 2), so they gate on `!offline &&
 * !isConnected` — enabled whenever offline, or the link is strictly
 * `connected`; only an *online* tune loses them mid-reconnect. Write-to-ECU
 * (Task 6) is the mirror case, gated on `!offline || !isConnected`: it only
 * makes sense for an offline-origin tune pushed onto a live, strictly-
 * connected link.
 */
export function TunePanel({ locale }: { locale: Locale }) {
  const connectionState = useConnectionStore((s) => s.connectionState);
  const isConnected = connectionState?.type === "connected";
  const linkAlive = isLinkAlive(connectionState);

  const definition = useTuneStore((s) => s.definition);
  const offline = useTuneStore((s) => s.offline);
  const values = useTuneStore((s) => s.values);
  const dirty = useTuneStore((s) => s.dirty);
  const activeDialog = useTuneStore((s) => s.activeDialog);
  const activeTable = useTuneStore((s) => s.activeTable);
  const activeCurve = useTuneStore((s) => s.activeCurve);

  const [conditions, setConditions] = useState<Record<string, boolean>>({});
  const [error, setError] = useState<string | null>(null);
  const refreshGeneration = useRef(0);

  // Re-read all values + re-evaluate all conditions from the backend.
  const refresh = useCallback(async (def: DefinitionDto) => {
    const generation = ++refreshGeneration.current;
    const names = def.constants.map((c) => c.name);
    const valuesRes = await commands.getValues(names);
    if (valuesRes.status === "ok" && generation === refreshGeneration.current) {
      const map: Record<string, (typeof valuesRes.data)[number]> = {};
      names.forEach((name, i) => (map[name] = valuesRes.data[i]));
      useTuneStore.getState().setValues(map);
    }
    const boundsRes = await commands.resolveGaugeBounds();
    if (boundsRes.status === "ok" && generation === refreshGeneration.current) {
      useTuneStore.getState().setGaugeBounds(boundsRes.data);
    }
    const exprs = conditionExprs(def);
    if (exprs.length > 0) {
      const condRes = await commands.evalConditions(exprs);
      if (condRes.status === "ok" && generation === refreshGeneration.current) {
        const map: Record<string, boolean> = {};
        exprs.forEach((expr, i) => (map[expr] = condRes.data[i]));
        setConditions(map);
      }
    }
  }, []);

  // Load definition + tune on a fresh online connect; otherwise just refresh
  // values/conditions (wire-free, so it also serves the offline case: a
  // definition already present — via `OfflinePanel`'s `setOfflineDefinition`,
  // or a prior connect — is re-read but never re-fetched or reloaded, which
  // would overwrite offline edits on attach). Gating the fetch branch on
  // `linkAlive` rather than `isConnected` means a `reconnecting` glitch
  // neither re-fetches nor reloads: `reconnecting` only ever follows
  // `connected`, so becoming alive always means becoming connected, and
  // staying alive through connected → reconnecting → connected leaves this
  // effect a no-op — definition/values/dirty simply survive the blip.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      const store = useTuneStore.getState();
      if (store.definition) {
        // Definition already present (offline via OfflinePanel, or a prior
        // connect). Re-read values + conditions — but only when there is
        // somewhere to read them from: a live link, or an offline tune (no
        // wire needed). On a true disconnect of an *online* tune, `definition`
        // is still set here (this effect runs before the sibling reset effect
        // commits `reset()`), so without this guard we'd fire a doomed
        // `getValues`/`evalConditions` at the dead link. Never reload the
        // tune here either way — that would overwrite offline edits on attach.
        if (!cancelled && (linkAlive || store.offline)) {
          await refresh(store.definition);
        }
        return;
      }
      if (!linkAlive) return; // nothing loaded yet and no link — show nothing
      const defRes = await commands.getDefinition();
      if (defRes.status !== "ok" || cancelled) {
        if (defRes.status === "error") setError(defRes.error);
        return;
      }
      const def = defRes.data;
      store.setDefinition(def);
      const firstDialog =
        def.menus[0]?.items[0]?.dialog ?? def.dialogs[0]?.name ?? null;
      store.setActiveDialog(firstDialog);
      const loadRes = await commands.loadTune();
      if (loadRes.status === "error") {
        if (!cancelled) {
          useTuneStore.getState().reset();
          setError(loadRes.error);
        }
        return;
      }
      if (!cancelled) {
        useTuneStore.getState().setDefinition(def);
        const firstDialog =
          def.menus[0]?.items[0]?.dialog ?? def.dialogs[0]?.name ?? null;
        useTuneStore.getState().setActiveDialog(firstDialog);
        await refresh(def);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [linkAlive, definition, refresh]);

  // Reset on a *true* disconnect — but only for a live-read tune. A
  // file-backed offline tune (via `OfflinePanel`) has no wire link to lose,
  // so it survives so the user can keep editing after unplugging.
  useEffect(() => {
    if (!linkAlive && !useTuneStore.getState().offline) {
      useTuneStore.getState().reset();
    }
  }, [linkAlive]);

  // Reflect backend dirty-state events into the store.
  useEffect(() => {
    const unlisten = events.tuneDirtyEvent.listen((e) =>
      useTuneStore.getState().applyDirty(e.payload),
    );
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  const onEdit = useCallback(
    async (name: string, value: Value) => {
      setError(null);
      try {
        await useTuneStore.getState().setValue(name, value);
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      }
      // Re-sync values + conditions from the backend (source of truth) whether
      // the write succeeded (values may have been rounded/clamped) or failed
      // (the store rolled back optimistically; confirm against the ECU).
      if (definition) await refresh(definition);
    },
    [definition, refresh],
  );

  const runAndRefresh = useCallback(
    async (op: () => Promise<{ status: "ok" | "error"; error?: string }>) => {
      setError(null);
      const res = await op();
      if (res.status === "error" && res.error) setError(res.error);
      if (definition) await refresh(definition);
    },
    [definition, refresh],
  );

  if (!definition) {
    return null;
  }

  return (
    <section className="tune-panel" aria-label={t("tune.title", locale)}>
      <header className="tune-header">
        <h2>{t("tune.title", locale)}</h2>
        {dirty && (
          <span className="tune-badge" role="status">
            {t("tune.badge.modified", locale)}
          </span>
        )}
        {/* Burn always needs a live link (there is nothing to flash to
            offline), so it stays gated on `!dirty || !isConnected` — while
            merely `reconnecting` this correctly stays disabled too, since
            `isConnected` is strictly `connectionState.type === "connected"`.
            Undo/redo are wire-free for an offline tune (Task 2), so they are
            enabled whenever `offline` OR the link is strictly `connected` —
            `!offline && !isConnected` disables them only for an online tune
            while merely `reconnecting`, so they never put new traffic on a
            link being re-established. Write-to-ECU is the mirror case: it
            only makes sense for an offline-origin tune pushed onto a live,
            strictly-connected link, so it is gated on `!offline ||
            !isConnected`. Field edits and diff/merge actions are NOT gated
            here — the owner queues their commands behind the reconnect
            (safe, just delayed); gating them too is a recorded follow-up. */}
        <div className="tune-actions">
          <button
            type="button"
            onClick={() => runAndRefresh(() => commands.burnTune())}
            disabled={!dirty || !isConnected}
          >
            {t("tune.burn", locale)}
          </button>
          <button
            type="button"
            onClick={() => runAndRefresh(() => commands.undoTune())}
            disabled={!offline && !isConnected}
          >
            {t("tune.undo", locale)}
          </button>
          <button
            type="button"
            onClick={() => runAndRefresh(() => commands.redoTune())}
            disabled={!offline && !isConnected}
          >
            {t("tune.redo", locale)}
          </button>
          <button
            type="button"
            onClick={() => runAndRefresh(() => commands.writeTuneToEcu())}
            disabled={!offline || !isConnected}
          >
            {t("tune.writeToEcu", locale)}
          </button>
        </div>
      </header>

      {error && <p className="tune-error">{error}</p>}

      <div className="tune-body">
        <div className="tune-navs">
          <nav className="tune-menu" aria-label={t("tune.menu", locale)}>
            {definition.menus.flatMap((menu) =>
              menu.items.map((item) => (
                <button
                  key={item.dialog}
                  type="button"
                  className="tune-menu-item"
                  aria-current={activeDialog === item.dialog}
                  onClick={() => {
                    // A menu item may point at a dialog, a table editor (or
                    // its 3-D map id), or a curve — rusEFI menus do all four.
                    const target = resolveMenuTarget(definition, item.dialog);
                    const store = useTuneStore.getState();
                    if (target.kind === "table")
                      store.setActiveTable(target.name);
                    else if (target.kind === "curve")
                      store.setActiveCurve(target.name);
                    else store.setActiveDialog(target.name);
                  }}
                >
                  {item.label}
                </button>
              )),
            )}
          </nav>

          {definition.tables.length > 0 && (
            <nav className="tune-menu" aria-label={t("table.navLabel", locale)}>
              {definition.tables.map((table) => (
                <button
                  key={table.name}
                  type="button"
                  className="tune-menu-item"
                  aria-current={activeTable === table.name}
                  onClick={() =>
                    useTuneStore.getState().setActiveTable(table.name)
                  }
                >
                  {table.title || table.name}
                </button>
              ))}
            </nav>
          )}

          {/* Renders nothing against the bundled sim INI (no curves yet);
              content is `CurveEditor` (Task 6) below. */}
          {definition.curves.length > 0 && (
            <nav className="tune-menu" aria-label={t("curve.navLabel", locale)}>
              {definition.curves.map((curve) => (
                <button
                  key={curve.name}
                  type="button"
                  className="tune-menu-item"
                  aria-current={activeCurve === curve.name}
                  onClick={() =>
                    useTuneStore.getState().setActiveCurve(curve.name)
                  }
                >
                  {curve.title || curve.name}
                </button>
              ))}
            </nav>
          )}
        </div>

        <div className="tune-content">
          {activeTable ? (
            <TableEditor locale={locale} />
          ) : activeCurve ? (
            <CurveEditor locale={locale} />
          ) : activeDialog ? (
            <DialogEngine
              definition={definition}
              dialogName={activeDialog}
              values={values}
              conditions={conditions}
              onEdit={onEdit}
              locale={locale}
            />
          ) : (
            <p>{t("tune.noDialog", locale)}</p>
          )}
        </div>
      </div>

      <TuneDiff
        locale={locale}
        onAfterMerge={() =>
          definition ? refresh(definition) : Promise.resolve()
        }
      />
    </section>
  );
}
