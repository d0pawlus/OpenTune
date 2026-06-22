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

  const connectionState = useConnectionStore((s) => s.connectionState);
  const isConnected = connectionState?.type === "connected";

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
    if (!selectedPort) {
      return;
    }
    // M1: placeholder. The actual connection logic will be wired through
    // the backend protocol engine in a follow-up CL. For now, just
    // demo the state transitions.
    // TODO(m1-wiring): wire to backend connect command
    console.log(`Connecting to ${selectedPort}`);
  };

  const handleDisconnect = async () => {
    // M1: placeholder; wired in follow-up CL
    // TODO(m1-wiring): wire to backend disconnect command
    console.log("Disconnecting");
  };

  const stateText = getConnectionStateText(connectionState, locale);
  const signature =
    connectionState?.type === "connected" ? connectionState.signature : "—";
  const version =
    connectionState?.type === "connected" ? connectionState.version : "—";

  return (
    <section>
      <h2>{t("connect.title", locale)}</h2>

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
              <option disabled>{t("connect.noPortsAvailable", locale)}</option>
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

      <div>
        <button
          onClick={handleConnect}
          disabled={isConnected || !selectedPort || loadingPorts}
        >
          {t("connect.connect", locale)}
        </button>
        <button onClick={handleDisconnect} disabled={!isConnected}>
          {t("connect.disconnect", locale)}
        </button>
      </div>

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
