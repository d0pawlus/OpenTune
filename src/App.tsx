// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useState } from "react";
import { commands, type AppInfo } from "./ipc/bindings";

function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  useEffect(() => {
    commands.appInfo().then(setInfo);
  }, []);
  return <main>{info ? `${info.name} v${info.version}` : "…"}</main>;
}

export default App;
