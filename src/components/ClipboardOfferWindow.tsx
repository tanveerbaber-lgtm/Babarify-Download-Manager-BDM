import { useCallback, useEffect, useState } from "react";
import { Download, X } from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { api, EVENTS } from "../lib/api";
import type { ClipboardOffer } from "../lib/types";
import { Logo } from "./Logo";

/** The offer closes by itself if it's ignored this long. */
const AUTO_DISMISS_MS = 12_000;

/** "Download with BDM?" shown in the corner when a link to a file is copied. */
export function ClipboardOfferWindow() {
  const [offer, setOffer] = useState<ClipboardOffer | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [shownAt, setShownAt] = useState(Date.now());

  const load = useCallback(async () => {
    setOffer(await api.getClipboardOffer());
    setError(null);
    setShownAt(Date.now());
  }, []);

  useEffect(() => {
    void load();
    const stop = listen(EVENTS.offerChanged, () => void load());
    return () => void stop.then((s) => s());
  }, [load]);

  // Restarted whenever a newer link replaces the shown one.
  useEffect(() => {
    if (busy || error) return;
    const timer = setTimeout(() => void api.dismissClipboardOffer(), AUTO_DISMISS_MS);
    return () => clearTimeout(timer);
  }, [shownAt, busy, error]);

  const accept = async () => {
    setBusy(true);
    try {
      await api.acceptClipboardOffer();
    } catch (e) {
      setError((e as { message?: string }).message ?? String(e));
      setBusy(false);
    }
  };

  return (
    <div className="offer" role="dialog" aria-label="Download with BDM?">
      <Logo size={30} />
      <div className="offer-text">
        <div className="offer-title">Download with BDM?</div>
        <div className="offer-name" title={offer?.url}>{offer?.fileName ?? "…"}</div>
        <div className={error ? "offer-error" : "offer-host"}>{error ?? offer?.host}</div>
      </div>
      <div className="offer-actions">
        <button type="button" className="button primary small" onClick={() => void accept()} disabled={!offer || busy}>
          <Download size={14} aria-hidden />
          Download
        </button>
        <button type="button" className="icon-button" title="Dismiss" aria-label="Dismiss" onClick={() => void api.dismissClipboardOffer()}>
          <X size={15} aria-hidden />
        </button>
      </div>
    </div>
  );
}
