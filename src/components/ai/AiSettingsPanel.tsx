// SPDX-License-Identifier: GPL-3.0-or-later
import { useCallback, useEffect, useState } from "react";
import { commands, type AiSettingsDto } from "../../ipc/bindings";
import { t, type Locale } from "../../i18n";
import "./ai.css";

const AI_PROVIDERS = ["anthropic", "openai"] as const;

type KeyPresence = "unknown" | "present" | "missing";

export function AiSettingsPanel({ locale }: { locale: Locale }) {
  const [settings, setSettings] = useState<AiSettingsDto | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [keyPresence, setKeyPresence] = useState<KeyPresence>("unknown");
  const [apiKey, setApiKey] = useState("");
  const [savingSettings, setSavingSettings] = useState(false);
  const [savingKey, setSavingKey] = useState(false);
  const [clearingKey, setClearingKey] = useState(false);
  const [saved, setSaved] = useState(false);

  // Re-queried on mount, on provider change, and after save/clear key — the
  // key-present flag is per-provider and must never go stale across those.
  const refreshKeyPresence = useCallback(async (provider: string) => {
    const result = await commands.aiKeyPresent(provider);
    if (result.status === "ok") {
      setKeyPresence(result.data ? "present" : "missing");
    } else {
      setError(result.error);
    }
  }, []);

  useEffect(() => {
    let active = true;
    void (async () => {
      const result = await commands.getAiSettings();
      if (!active) return;
      if (result.status === "ok") {
        setSettings(result.data);
        await refreshKeyPresence(result.data.provider);
      } else {
        setError(result.error);
      }
    })();
    return () => {
      active = false;
    };
  }, [refreshKeyPresence]);

  const updateSettings = (patch: Partial<AiSettingsDto>) => {
    setSaved(false);
    setSettings((prev) => (prev ? { ...prev, ...patch } : prev));
  };

  const handleProviderChange = (provider: string) => {
    updateSettings({ provider });
    setKeyPresence("unknown");
    void refreshKeyPresence(provider);
  };

  const handleSaveSettings = async () => {
    if (!settings) return;
    setSavingSettings(true);
    setError(null);
    const result = await commands.setAiSettings(settings);
    setSavingSettings(false);
    if (result.status === "ok") {
      setSaved(true);
    } else {
      setError(result.error);
    }
  };

  const handleSaveKey = async () => {
    if (!settings || apiKey.length === 0) return;
    setSavingKey(true);
    setError(null);
    const result = await commands.setAiKey(settings.provider, apiKey);
    setSavingKey(false);
    if (result.status === "ok") {
      setApiKey("");
      await refreshKeyPresence(settings.provider);
    } else {
      setError(result.error);
    }
  };

  const handleClearKey = async () => {
    if (!settings) return;
    setClearingKey(true);
    setError(null);
    const result = await commands.clearAiKey(settings.provider);
    setClearingKey(false);
    if (result.status === "ok") {
      await refreshKeyPresence(settings.provider);
    } else {
      setError(result.error);
    }
  };

  return (
    <section className="ai-settings" aria-labelledby="ai-settings-title">
      <header>
        <h2 id="ai-settings-title">{t("ai.title", locale)}</h2>
      </header>

      {error && (
        <p role="alert" className="ai-error">
          {error}
        </p>
      )}

      {settings && (
        <>
          <div className="ai-field">
            <label>
              <input
                type="checkbox"
                checked={settings.enabled}
                onChange={(e) => updateSettings({ enabled: e.target.checked })}
              />{" "}
              {t("ai.enable", locale)}
            </label>
            <p className="ai-consent">{t("ai.consent", locale)}</p>
          </div>

          <div className="ai-field">
            <label>
              {t("ai.provider", locale)}
              <select
                value={settings.provider}
                onChange={(e) => handleProviderChange(e.target.value)}
              >
                {AI_PROVIDERS.map((provider) => (
                  <option key={provider} value={provider}>
                    {provider}
                  </option>
                ))}
              </select>
            </label>
          </div>

          <div className="ai-field">
            <label>
              {t("ai.model", locale)}
              <input
                type="text"
                value={settings.model}
                onChange={(e) => updateSettings({ model: e.target.value })}
              />
            </label>
          </div>

          <div className="ai-actions">
            <button
              type="button"
              aria-busy={savingSettings}
              disabled={savingSettings}
              onClick={() => void handleSaveSettings()}
            >
              {t("ai.saveSettings", locale)}
            </button>
            {saved && (
              <p role="status" className="ai-saved">
                {t("ai.saved", locale)}
              </p>
            )}
          </div>

          <div className="ai-field">
            <label>
              {t("ai.apiKey", locale)}
              <input
                type="password"
                value={apiKey}
                onChange={(e) => setApiKey(e.target.value)}
                autoComplete="off"
              />
            </label>
          </div>

          <div className="ai-actions">
            <button
              type="button"
              aria-busy={savingKey}
              disabled={savingKey || apiKey.length === 0}
              onClick={() => void handleSaveKey()}
            >
              {t("ai.saveKey", locale)}
            </button>
            <button
              type="button"
              aria-busy={clearingKey}
              disabled={clearingKey || keyPresence !== "present"}
              onClick={() => void handleClearKey()}
            >
              {t("ai.clearKey", locale)}
            </button>
          </div>

          {keyPresence !== "unknown" && (
            <p role="status" className="ai-key-status">
              {keyPresence === "present"
                ? t("ai.keyPresent", locale)
                : t("ai.keyMissing", locale)}
            </p>
          )}
        </>
      )}
    </section>
  );
}
