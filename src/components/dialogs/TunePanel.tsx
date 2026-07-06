// SPDX-License-Identifier: GPL-3.0-or-later
import { useCallback, useEffect, useState } from "react";
import { commands, events } from "../../ipc/bindings";
import type { DefinitionDto, Value } from "../../ipc/bindings";
import { isLinkAlive, useConnectionStore } from "../../stores/connection";
import { useTuneStore } from "../../stores/tune";
import { t, type Locale } from "../../i18n";
import { DialogEngine } from "./DialogEngine";
import { TableEditor } from "../table-editor/TableEditor";
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
 * too. The load/reset effect and the render gate therefore use
 * {@link isLinkAlive} (`connected` or `reconnecting`), matching `Dashboard`'s
 * mount predicate: the panel stays visible through a glitch and only a true
 * disconnect (`disconnected`/`failed`) resets the store. Wire-touching
 * actions stay connected-only: the load/refresh sequence runs only when the
 * link *becomes* alive (i.e. on connect — `reconnecting` only ever follows
 * `connected`, so it never re-fires mid-glitch), and burn/undo/redo are
 * disabled while merely reconnecting.
 */
export function TunePanel({ locale }: { locale: Locale }) {
  const connectionState = useConnectionStore((s) => s.connectionState);
  const isConnected = connectionState?.type === "connected";
  const linkAlive = isLinkAlive(connectionState);

  const definition = useTuneStore((s) => s.definition);
  const values = useTuneStore((s) => s.values);
  const dirty = useTuneStore((s) => s.dirty);
  const activeDialog = useTuneStore((s) => s.activeDialog);
  const activeTable = useTuneStore((s) => s.activeTable);
  const activeCurve = useTuneStore((s) => s.activeCurve);

  const [conditions, setConditions] = useState<Record<string, boolean>>({});
  const [error, setError] = useState<string | null>(null);

  // Re-read all values + re-evaluate all conditions from the backend.
  const refresh = useCallback(async (def: DefinitionDto) => {
    const names = def.constants.map((c) => c.name);
    const valuesRes = await commands.getValues(names);
    if (valuesRes.status === "ok") {
      const map: Record<string, (typeof valuesRes.data)[number]> = {};
      names.forEach((name, i) => (map[name] = valuesRes.data[i]));
      useTuneStore.getState().setValues(map);
    }
    const exprs = conditionExprs(def);
    if (exprs.length > 0) {
      const condRes = await commands.evalConditions(exprs);
      if (condRes.status === "ok") {
        const map: Record<string, boolean> = {};
        exprs.forEach((expr, i) => (map[expr] = condRes.data[i]));
        setConditions(map);
      }
    }
  }, []);

  // Load definition + tune once the link comes alive. On a true disconnect
  // we reset the store (an external system) and let the panel unmount its
  // content; stale local `conditions` are harmless (the panel renders null)
  // and are fully replaced by `refresh` on the next connect. Gating on
  // `linkAlive` rather than `isConnected` means a `reconnecting` glitch
  // neither resets the store nor re-fetches: `reconnecting` only ever
  // follows `connected`, so becoming alive always means becoming connected,
  // and staying alive through connected → reconnecting → connected leaves
  // this effect untouched — definition/values/dirty simply survive the blip.
  useEffect(() => {
    if (!linkAlive) {
      useTuneStore.getState().reset();
      return;
    }
    let cancelled = false;
    (async () => {
      const defRes = await commands.getDefinition();
      if (defRes.status !== "ok" || cancelled) {
        if (defRes.status === "error") setError(defRes.error);
        return;
      }
      const def = defRes.data;
      useTuneStore.getState().setDefinition(def);
      const firstDialog =
        def.menus[0]?.items[0]?.dialog ?? def.dialogs[0]?.name ?? null;
      useTuneStore.getState().setActiveDialog(firstDialog);

      const loadRes = await commands.loadTune();
      if (loadRes.status === "error") {
        setError(loadRes.error);
        return;
      }
      if (!cancelled) await refresh(def);
    })();
    return () => {
      cancelled = true;
    };
  }, [linkAlive, refresh]);

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

  if (!linkAlive || !definition) {
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
        {/* Burn/undo/redo are connected-only: while `reconnecting` the
            panel stays visible (see the component doc) but these buttons
            must not put new traffic on a link being re-established. Field
            edits and diff/merge actions are NOT gated here — the owner
            queues their commands behind the reconnect (safe, just delayed);
            gating them too is a recorded follow-up. */}
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
            disabled={!isConnected}
          >
            {t("tune.undo", locale)}
          </button>
          <button
            type="button"
            onClick={() => runAndRefresh(() => commands.redoTune())}
            disabled={!isConnected}
          >
            {t("tune.redo", locale)}
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
                  onClick={() =>
                    useTuneStore.getState().setActiveDialog(item.dialog)
                  }
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

          {/* Renders nothing until Task 6 — the sim INI has no curves yet. */}
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
          ) : activeDialog ? (
            <DialogEngine
              definition={definition}
              dialogName={activeDialog}
              values={values}
              conditions={conditions}
              onEdit={onEdit}
            />
          ) : (
            <p>{t("tune.noDialog", locale)}</p>
          )}
        </div>
      </div>

      <TuneDiff locale={locale} />
    </section>
  );
}
