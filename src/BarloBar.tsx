import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./BarloBar.css";

interface StatusBarApp {
  pid: number;
  name: string;
  bundle_id: string;
}

export function BarloBar() {
  const [apps, setApps] = useState<StatusBarApp[]>([]);

  useEffect(() => {
    invoke<StatusBarApp[]>("get_status_bar_apps")
      .then(setApps)
      .catch(() => {});
  }, []);

  return (
    <div className="bar">
      {apps.map((app) => (
        <BarloBarItem key={app.pid} app={app} />
      ))}
    </div>
  );
}

function BarloBarItem({ app }: { app: StatusBarApp }) {
  return (
    <div className="bar-item" title={app.name}>
      <span className="bar-item-label">{app.name.charAt(0)}</span>
    </div>
  );
}
