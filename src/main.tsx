import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

// For the barlo-bar window, force transparent background before React renders.
// This prevents WebKit from locking in an opaque background based on the body color.
if (getCurrentWebviewWindow().label === "barlo-bar") {
  document.documentElement.style.setProperty("background", "transparent", "important");
  document.body.style.setProperty("background", "transparent", "important");
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
