// SPDX-License-Identifier: GPL-3.0-or-later
import { useCallback, useEffect, useState } from "react";
import {
  commands,
  type PortInfoDto,
  type ConnectionStateEvent,
} from "../ipc/bindings";
import { useConnectionStore } from "../stores/connection";
import { t, type Locale } from "../i18n";

interface ConnectProps {
  locale: Locale;
}

function getConnectionStateText(
  state: ConnectionStateEvent | null,
  locale: Locale,
): string {
  if (state === null) {
    return "—";
  }
  switch (state.type) {
    case "disconnected":
      return t("connection.disconnected", locale);
    case "connecting":
      return t("connection.connecting", locale);
    case "connected":
      return t("connection.connected", locale);
    case "reconnecting":
      return `${t("connection.reconnecting", locale)} (${state.attempt})`;
    case "failed":
      return t("connection.failed", locale);
    default:
      return "—";
  }
}

export function Connect({ locale }: ConnectProps) {
  const [ports, setPorts] = useState<PortInfoDto[]>([]);
  const [selectedPort, setSelectedPort] = useState<string>("");
  const [loadingPorts, setLoadingPorts] = useState(false);
  const [useSimulator, setUseSimulator] = useState(false);
  const [iniPath, setIniPath] = useState<string>("");
  const [error, setError] = useState<string | null>(null);

  const connectionState = useConnectionStore((s) => s.connectionState);
  const isConnected =
    connectionState?.type === "connected" ||
    connectionState?.type === "connecting" ||
    connectionState?.type === "reconnecting";
  const isSimConnected = isConnected && useSimulator;

  const refreshPorts = useCallback(async () => {
    setLoadingPorts(true);
    try {
      const result = await commands.listPorts();
      if (result.status === "ok") {
        setPorts(result.data);
        if (result.data.length > 0 && !selectedPort) {
          setSelectedPort(result.data[0].name);
        }
      }
    } finally {
      setLoadingPorts(false);
    }
  }, [selectedPort]);

  useEffect(() => {
    const loadPorts = async () => {
      await refreshPorts();
    };
    void loadPorts();
  }, [refreshPorts]);

  const handleConnect = async () => {
    setError(null);
    const source = useSimulator
      ? ({ type: "simulator", ini_path: null } as const)
      : ({
          type: "serial",
          port_name: selectedPort,
          ini_path: iniPath,
        } as const);

    const result = await commands.connect(source);
    if (result.status === "error") {
      setError(result.error);
    }
  };

  const handleDisconnect = async () => {
    setError(null);
    const result = await commands.disconnect();
    if (result.status === "error") {
      setError(result.error);
    }
  };

  const handleSimulateLinkDrop = async () => {
    setError(null);
    const result = await commands.simulateLinkDrop();
    if (result.status === "error") {
      setError(result.error);
    }
  };

  const stateText = getConnectionStateText(connectionState, locale);
  const signature =
    connectionState?.type === "connected" ? connectionState.signature : "—";
  const version =
    connectionState?.type === "connected" ? connectionState.version : "—";

  const canConnect = useSimulator
    ? !isConnected
    : !isConnected && !!selectedPort && !loadingPorts;

  return (
    <section>
      <h2>{t("connect.title", locale)}</h2>

      <div>
        <label>
          <input
            type="checkbox"
            checked={useSimulator}
            onChange={(e) => setUseSimulator(e.target.checked)}
            disabled={isConnected}
          />{" "}
          {t("connect.useSimulator", locale)}
        </label>
      </div>

      {!useSimulator && (
        <div>
          <label>
            {t("connect.selectPort", locale)}
            <select
              value={selectedPort}
              onChange={(e) => setSelectedPort(e.target.value)}
              disabled={isConnected || loadingPorts}
            >
              <option value="">{t("connect.portPlaceholder", locale)}</option>
              {ports.length === 0 ? (
                <option disabled>
                  {t("connect.noPortsAvailable", locale)}
                </option>
              ) : (
                ports.map((port) => (
                  <option key={port.name} value={port.name}>
                    {port.name} {port.product ? `(${port.product})` : ""}
                  </option>
                ))
              )}
            </select>
          </label>
          <button onClick={refreshPorts} disabled={isConnected || loadingPorts}>
            {t("connect.refreshPorts", locale)}
          </button>
        </div>
      )}

      {/* INI is serial-only: the simulator uses a bundled Speeduino INI that
          matches it, so a custom (e.g. rusEFI) INI would only mismatch. */}
      {!useSimulator && (
        <div>
          <label>
            {t("connect.selectIni", locale)}
            <input
              type="text"
              value={iniPath}
              onChange={(e) => setIniPath(e.target.value)}
              placeholder={t("connect.iniPlaceholder", locale)}
              disabled={isConnected}
            />
          </label>
        </div>
      )}

      <div>
        <button onClick={handleConnect} disabled={!canConnect}>
          {useSimulator
            ? t("connect.connectSimulator", locale)
            : t("connect.connect", locale)}
        </button>
        <button onClick={handleDisconnect} disabled={!isConnected}>
          {t("connect.disconnect", locale)}
        </button>
        {isSimConnected && (
          <button onClick={handleSimulateLinkDrop}>
            {t("connect.simulateDrop", locale)}
          </button>
        )}
      </div>

      {error !== null && (
        <p role="alert" style={{ color: "red" }}>
          {error}
        </p>
      )}

      <div>
        <p>
          <strong>{t("connect.connectionState", locale)}:</strong> {stateText}
        </p>
        <p>
          <strong>{t("connect.signature", locale)}:</strong> {signature}
        </p>
        <p>
          <strong>{t("connect.version", locale)}:</strong> {version}
        </p>
      </div>
    </section>
  );
}
