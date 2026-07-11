// SPDX-License-Identifier: GPL-3.0-or-later
import { useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { commands } from "../../ipc/bindings";
import type { DefinitionDto } from "../../ipc/bindings";
import { useTuneStore } from "../../stores/tune";
import { t, type Locale } from "../../i18n";
import "./offline.css";

async function pickFile(name: string, ext: string): Promise<string | null> {
  const picked = await open({
    multiple: false,
    filters: [{ name, extensions: [ext] }],
  });
  return typeof picked === "string" ? picked : null;
}

function loadDefinition(def: DefinitionDto): void {
  const store = useTuneStore.getState();
  store.setOfflineDefinition(def);
  const firstDialog =
    def.menus[0]?.items[0]?.dialog ?? def.dialogs[0]?.name ?? null;
  store.setActiveDialog(firstDialog);
}

/**
 * The pre-link entry surface: pick an INI to start a blank offline tune,
 * open an existing `.msq` against an INI, or save the current (possibly
 * offline) tune to a `.msq`. Loaded definitions land in `useTuneStore` via
 * `setOfflineDefinition`, which flips the store's `offline` flag so
 * `TunePanel` renders with no wire link and survives a later disconnect.
 */
export function OfflinePanel({ locale }: { locale: Locale }) {
  const [error, setError] = useState<string | null>(null);
  const hasTune = useTuneStore((s) => s.definition !== null);

  const newTune = async () => {
    setError(null);
    const ini = await pickFile("INI", "ini");
    if (!ini) return;
    const res = await commands.newTune(ini);
    if (res.status === "error") return setError(res.error);
    loadDefinition(res.data);
  };

  const openTune = async () => {
    setError(null);
    const ini = await pickFile("INI", "ini");
    if (!ini) return;
    const msq = await pickFile("Tune", "msq");
    if (!msq) return;
    const res = await commands.openTune(ini, msq);
    if (res.status === "error") return setError(res.error);
    loadDefinition(res.data);
  };

  const saveTune = async () => {
    setError(null);
    const path = await save({
      filters: [{ name: "Tune", extensions: ["msq"] }],
    });
    if (typeof path !== "string") return;
    const res = await commands.saveTune(path);
    if (res.status === "error") setError(res.error);
  };

  return (
    <section className="offline-panel" aria-label={t("offline.title", locale)}>
      <h2>{t("offline.title", locale)}</h2>
      <div className="offline-actions">
        <button type="button" onClick={newTune}>
          {t("offline.new", locale)}
        </button>
        <button type="button" onClick={openTune}>
          {t("offline.open", locale)}
        </button>
        <button type="button" onClick={saveTune} disabled={!hasTune}>
          {t("offline.save", locale)}
        </button>
      </div>
      {error && <p className="offline-error">{error}</p>}
    </section>
  );
}
