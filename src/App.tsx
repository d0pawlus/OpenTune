// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useState } from "react";
import { commands, events, type AppInfo } from "./ipc/bindings";
import { useConnectionStore } from "./stores/connection";

function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const lastSeq = useConnectionStore((s) => s.lastSeq);

  useEffect(() => {
    commands.appInfo().then(setInfo);
  }, []);

  useEffect(() => {
    const unlisten = events.heartbeat.listen((e) =>
      useConnectionStore.getState().setSeq(e.payload.seq),
    );
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  return (
    <main>
      {info ? `${info.name} v${info.version}` : "…"}
      <p>heartbeat: {lastSeq ?? "—"}</p>
    </main>
  );
}

export default App;
