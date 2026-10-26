import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { DownloadWindow } from "./components/DownloadWindow";
import { ClipboardOfferWindow } from "./components/ClipboardOfferWindow";
import "./styles/tokens.css";
import "./styles/app.css";

const label = (window as { __TAURI_INTERNALS__?: { metadata?: { currentWindow?: { label?: string } } } }).__TAURI_INTERNALS__?.metadata?.currentWindow?.label ?? "";

/** Download windows are "download-<id>", the copied-link offer is "clipboard-offer", the rest is the main window. */
function view() {
  if (label.startsWith("download-")) return <DownloadWindow id={label.slice("download-".length)} />;
  if (label === "clipboard-offer") return <ClipboardOfferWindow />;
  return <App />;
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(<React.StrictMode>{view()}</React.StrictMode>);
