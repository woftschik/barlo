import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import "./App.css";

function getWindowLabel(): string {
  try {
    return getCurrentWebviewWindow().label;
  } catch {
    return "settings";
  }
}

interface StatusBarApp {
  pid: number;
  name: string;
  bundle_id: string;
}

// ---- Barlo Bar View ----
function BarloBarView() {
  const [apps, setApps] = useState<StatusBarApp[]>([]);

  useEffect(() => {
    invoke<StatusBarApp[]>("get_status_bar_apps").then(setApps).catch(() => {});
  }, []);

  return (
    <div className="barlo-bar">
      <div className="barlo-bar-label">Barlo Bar</div>
      <div className="barlo-bar-items">
        {apps.slice(0, 8).map((app) => (
          <div key={app.pid} className="barlo-bar-item" title={app.name}>
            {app.name.charAt(0)}
          </div>
        ))}
      </div>
    </div>
  );
}

// ---- Settings View ----
function SettingsView() {
  const [apps, setApps] = useState<StatusBarApp[]>([]);
  const [hasAccessibility, setHasAccessibility] = useState(false);
  const [barloBarVisible, setBarloBarVisible] = useState(false);
  const [hiddenPids, setHiddenPids] = useState<Set<number>>(new Set());
  const [loading, setLoading] = useState(true);

  const refresh = async () => {
    setLoading(true);
    try {
      const [appList, accessibility] = await Promise.all([
        invoke<StatusBarApp[]>("get_status_bar_apps"),
        invoke<boolean>("check_accessibility"),
      ]);
      setApps(appList);
      setHasAccessibility(accessibility);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    refresh();
  }, []);

  const handleToggleBarloBar = async () => {
    const next = !barloBarVisible;
    await invoke("toggle_barlo_bar", { visible: next });
    setBarloBarVisible(next);
  };

  const handleToggleApp = (pid: number) => {
    setHiddenPids((prev: Set<number>) => {
      const next = new Set(prev);
      if (next.has(pid)) {
        next.delete(pid);
      } else {
        next.add(pid);
      }
      return next;
    });
  };

  return (
    <div className="settings">
      <div className="titlebar-drag" />

      <div className="settings-header">
        <div className="app-icon">B</div>
        <div>
          <h1>Barlo</h1>
          <p className="subtitle">Menu Bar Manager</p>
        </div>
      </div>

      {/* Barlo Bar */}
      <section className="card">
        <h2>Barlo Bar</h2>
        <p className="description">
          Eine zusätzliche Leiste direkt unter der Menüleiste, in der versteckte Icons angezeigt werden.
        </p>
        <div className="toggle-row">
          <span>Barlo Bar anzeigen</span>
          <button
            className={`toggle ${barloBarVisible ? "toggle-on" : ""}`}
            onClick={handleToggleBarloBar}
          >
            <span className="toggle-thumb" />
          </button>
        </div>
      </section>

      {/* Accessibility */}
      <section className="card">
        <h2>Berechtigungen</h2>
        <div className="status-row">
          <span className="status-label">Bedienungshilfen</span>
          <span className={`badge ${hasAccessibility ? "badge-active" : "badge-warning"}`}>
            {hasAccessibility ? "Erteilt" : "Erforderlich"}
          </span>
        </div>
        {!hasAccessibility && (
          <>
            <p className="description warning-text" style={{ marginTop: 8 }}>
              Bedienungshilfen-Zugriff wird benötigt, um Menüleisten-Icons zu verwalten.
            </p>
            <button className="btn-primary" onClick={() => invoke("request_accessibility")}>
              Systemeinstellungen öffnen
            </button>
          </>
        )}
      </section>

      {/* Running Apps */}
      <section className="card">
        <div className="section-header">
          <h2>Menüleisten-Apps</h2>
          <button className="btn-ghost" onClick={refresh}>
            Aktualisieren
          </button>
        </div>
        <p className="description">
          Wähle, welche App-Icons in der Barlo Bar angezeigt werden sollen.
        </p>
        <div className="app-list">
          {loading ? (
            <div className="loading">Lade Apps...</div>
          ) : apps.length === 0 ? (
            <div className="loading">Keine Apps gefunden</div>
          ) : (
            apps.map((app) => (
              <div key={app.pid} className="app-row">
                <div className="app-info">
                  <div className="app-avatar">{app.name.charAt(0)}</div>
                  <div>
                    <div className="app-name">{app.name}</div>
                    <div className="app-bundle">{app.bundle_id || `PID ${app.pid}`}</div>
                  </div>
                </div>
                <button
                  className={`toggle ${hiddenPids.has(app.pid) ? "toggle-on" : ""}`}
                  onClick={() => handleToggleApp(app.pid)}
                >
                  <span className="toggle-thumb" />
                </button>
              </div>
            ))
          )}
        </div>
      </section>

      <div className="settings-footer">
        <span>Barlo v0.1.0</span>
      </div>
    </div>
  );
}

// ---- Root ----
function App() {
  const windowLabel = getWindowLabel();

  if (windowLabel === "barlo-bar") {
    return <BarloBarView />;
  }

  return <SettingsView />;
}

export default App;
