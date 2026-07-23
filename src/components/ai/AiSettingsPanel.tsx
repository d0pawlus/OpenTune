// SPDX-License-Identifier: GPL-3.0-or-later
import { useCallback, useEffect, useState } from "react";
import {
  commands,
  type AiSettingsDto,
  type McpStatusDto,
} from "../../ipc/bindings";
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
  const [mcpStatus, setMcpStatus] = useState<McpStatusDto | null>(null);
  // The token never sits in state until the user explicitly asks for it via
  // Show or Copy — it is the one secret that crosses IPC (see `mcpTokenInfo`
  // doc comment in bindings.ts) and must not be fetched on mount.
  const [mcpToken, setMcpToken] = useState<string | null>(null);
  const [mcpTokenRevealed, setMcpTokenRevealed] = useState(false);
  const [mcpTokenLoading, setMcpTokenLoading] = useState(false);
  const [mcpRegenerating, setMcpRegenerating] = useState(false);
  const [mcpCopied, setMcpCopied] = useState(false);
  const [mcpRegenerated, setMcpRegenerated] = useState(false);

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

  // Independent of the settings/key-presence load above: the MCP status
  // line reflects the server's actual running state, not the saved config.
  const refreshMcpStatus = useCallback(async () => {
    const result = await commands.mcpStatus();
    if (result.status === "ok") {
      setMcpStatus(result.data);
    } else {
      setError(result.error);
    }
  }, []);

  useEffect(() => {
    let active = true;
    void (async () => {
      if (active) {
        await refreshMcpStatus();
      }
    })();
    return () => {
      active = false;
    };
  }, [refreshMcpStatus]);

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
      // set_ai_settings reconciles the MCP server's start/stop/port-restart
      // backend-side (no separate start/stop call from this panel) — refresh
      // the status line so it reflects that reconciliation immediately.
      await refreshMcpStatus();
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

  // Shared by Show and Copy: fetches the token on first use only, then
  // reuses the cached value — Regenerate is the only path that overwrites
  // it with a fresh one.
  const ensureMcpToken = async (): Promise<string | null> => {
    if (mcpToken !== null) return mcpToken;
    setMcpTokenLoading(true);
    setError(null);
    const result = await commands.mcpTokenInfo(false);
    setMcpTokenLoading(false);
    if (result.status === "ok") {
      setMcpToken(result.data);
      return result.data;
    }
    setError(result.error);
    return null;
  };

  const handleShowToken = async () => {
    setMcpRegenerated(false);
    const value = await ensureMcpToken();
    if (value !== null) {
      setMcpTokenRevealed(true);
    }
  };

  const handleCopyToken = async () => {
    setMcpRegenerated(false);
    setMcpCopied(false);
    const value = await ensureMcpToken();
    if (value === null) return;
    try {
      await navigator.clipboard.writeText(value);
      setMcpCopied(true);
    } catch {
      setError(t("table.clipboardError", locale));
    }
  };

  const handleRegenerateToken = async () => {
    setMcpCopied(false);
    setMcpRegenerating(true);
    setError(null);
    const result = await commands.mcpTokenInfo(true);
    setMcpRegenerating(false);
    if (result.status === "ok") {
      setMcpToken(result.data);
      setMcpRegenerated(true);
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

          <div className="ai-mcp">
            <h3>{t("ai.mcp.title", locale)}</h3>

            <div className="ai-field">
              <label>
                <input
                  type="checkbox"
                  checked={settings.mcpEnabled}
                  onChange={(e) =>
                    updateSettings({ mcpEnabled: e.target.checked })
                  }
                />{" "}
                {t("ai.mcp.enable", locale)}
              </label>
            </div>

            <div className="ai-field">
              <label>
                {t("ai.mcp.port", locale)}
                <input
                  type="number"
                  min={1024}
                  value={settings.mcpPort}
                  onChange={(e) =>
                    updateSettings({ mcpPort: Number(e.target.value) })
                  }
                />
              </label>
            </div>

            {mcpStatus && (
              <p role="status" className="ai-mcp-status">
                {mcpStatus.running
                  ? t("ai.mcp.running", locale).replace(
                      "{port}",
                      String(mcpStatus.port),
                    )
                  : t("ai.mcp.stopped", locale)}
              </p>
            )}

            <div className="ai-field">
              <span className="ai-mcp-token-label">
                {t("ai.mcp.token", locale)}
              </span>
              <code className="ai-mcp-token-value">
                {mcpTokenRevealed && mcpToken ? mcpToken : "••••"}
              </code>
            </div>

            <div className="ai-actions">
              <button
                type="button"
                aria-busy={mcpTokenLoading}
                disabled={mcpTokenLoading || mcpRegenerating}
                onClick={() => void handleShowToken()}
              >
                {t("ai.mcp.show", locale)}
              </button>
              <button
                type="button"
                aria-busy={mcpTokenLoading}
                disabled={mcpTokenLoading || mcpRegenerating}
                onClick={() => void handleCopyToken()}
              >
                {t("ai.mcp.copy", locale)}
              </button>
              <button
                type="button"
                aria-busy={mcpRegenerating}
                disabled={mcpTokenLoading || mcpRegenerating}
                onClick={() => void handleRegenerateToken()}
              >
                {t("ai.mcp.regenerate", locale)}
              </button>
              {mcpCopied && (
                <p role="status" className="ai-mcp-copied">
                  {t("ai.mcp.copied", locale)}
                </p>
              )}
              {mcpRegenerated && (
                <p role="status" className="ai-mcp-regenerated">
                  {t("ai.mcp.regenerated", locale)}
                </p>
              )}
            </div>

            <p className="ai-mcp-hint">
              {t("ai.mcp.hint", locale)}{" "}
              <code>{`claude mcp add --transport http opentune http://127.0.0.1:${
                mcpStatus?.running ? mcpStatus.port : settings.mcpPort
              }/mcp --header "Authorization: Bearer ••••"`}</code>
            </p>
          </div>
        </>
      )}
    </section>
  );
}
