import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { DownloadWindow } from "./components/DownloadWindow";
import "@fontsource-variable/montserrat";
import "./styles/tokens.css";
import "./styles/app.css";

/** Download windows are labelled "download-<id>"; everything else is the main window. */
function downloadWindowId(): string | null {
  const label = (window as { __TAURI_INTERNALS__?: { metadata?: { currentWindow?: { label?: string } } } }).__TAURI_INTERNALS__?.metadata?.currentWindow?.label ?? "";
  return label.startsWith("download-") ? label.slice("download-".length) : null;
}

const downloadId = downloadWindowId();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>{downloadId ? <DownloadWindow id={downloadId} /> : <App />}</React.StrictMode>,
);
