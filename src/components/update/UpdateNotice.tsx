// SPDX-License-Identifier: GPL-3.0-or-later
import { useCallback, useEffect, useState } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { t, type Locale } from "../../i18n";
import "./update.css";

type UpdateState =
  | { kind: "checking" }
  | { kind: "idle"; checked: boolean }
  | { kind: "available"; update: Update }
  | { kind: "installing"; update: Update }
  | { kind: "error" };

async function requestUpdate(): Promise<UpdateState> {
  try {
    const update = await check();
    return update
      ? { kind: "available", update }
      : { kind: "idle", checked: true };
  } catch {
    return { kind: "error" };
  }
}

export function UpdateNotice({ locale }: { locale: Locale }) {
  const [state, setState] = useState<UpdateState>({ kind: "checking" });

  const checkForUpdate = useCallback(async () => {
    setState({ kind: "checking" });
    setState(await requestUpdate());
  }, []);

  useEffect(() => {
    let active = true;
    void requestUpdate().then((next) => {
      if (active) setState(next);
    });
    return () => {
      active = false;
    };
  }, []);

  const install = async (update: Update) => {
    setState({ kind: "installing", update });
    try {
      await update.downloadAndInstall();
      await relaunch();
    } catch {
      setState({ kind: "error" });
    }
  };

  if (state.kind === "available" || state.kind === "installing") {
    const { update } = state;
    return (
      <section className="update-notice" role="status">
        <p>
          {t("update.available", locale)} <strong>{update.version}</strong>
        </p>
        {update.body && <p className="update-notes">{update.body}</p>}
        <div className="update-actions">
          <button
            type="button"
            disabled={state.kind === "installing"}
            aria-busy={state.kind === "installing"}
            onClick={() => void install(update)}
          >
            {state.kind === "installing"
              ? t("update.installing", locale)
              : t("update.install", locale)}
          </button>
          <button
            type="button"
            disabled={state.kind === "installing"}
            onClick={() => setState({ kind: "idle", checked: false })}
          >
            {t("update.later", locale)}
          </button>
        </div>
      </section>
    );
  }

  return (
    <section className="update-control" aria-label={t("update.title", locale)}>
      {state.kind === "error" && (
        <p className="update-error" role="alert">
          {t("update.error", locale)}
        </p>
      )}
      {state.kind === "idle" && state.checked && (
        <p className="update-status" role="status">
          {t("update.none", locale)}
        </p>
      )}
      <button
        type="button"
        disabled={state.kind === "checking"}
        aria-busy={state.kind === "checking"}
        onClick={() => void checkForUpdate()}
      >
        {state.kind === "checking"
          ? t("update.checking", locale)
          : state.kind === "error"
            ? t("update.retry", locale)
            : t("update.check", locale)}
      </button>
    </section>
  );
}
