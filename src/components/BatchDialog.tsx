import { useEffect, useMemo, useState, type FormEvent } from "react";
import { open as openPicker } from "@tauri-apps/plugin-dialog";
import { FileUp, FolderOpen, X } from "lucide-react";
import { Modal } from "./Modal";
import { api } from "../lib/api";
import type { AppError, BatchRequest, Download, Queue } from "../lib/types";

type Mode = "pattern" | "list";

interface BatchDialogProps {
  open: boolean;
  queues: Queue[];
  onClose: () => void;
  onAdd: (request: BatchRequest) => Promise<Download[]>;
}

export function BatchDialog(props: BatchDialogProps) {
  return (
    <Modal open={props.open} title="Batch download" onClose={props.onClose} className="batch-dialog">
      <BatchForm {...props} />
    </Modal>
  );
}

/** Same link-picking rules as the Rust side: http(s) only, no duplicates. */
function linksIn(text: string): string[] {
  const seen = new Set<string>();
  for (const word of text.split(/\s+/)) {
    const w = word.replace(/^["'<]+|["'>,;]+$/g, "");
    if (/^https?:\/\/\S+$/i.test(w)) seen.add(w);
  }
  return [...seen];
}

function BatchForm({ queues, onClose, onAdd }: BatchDialogProps) {
  const [mode, setMode] = useState<Mode>("pattern");
  const [pattern, setPattern] = useState("");
  const [patternUrls, setPatternUrls] = useState<string[]>([]);
  const [listText, setListText] = useState("");
  const [destinationDir, setDestinationDir] = useState("");
  const [queueId, setQueueId] = useState("main");
  const [startNow, setStartNow] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // Live preview of the pattern.
  useEffect(() => {
    const p = pattern.trim();
    if (!p) {
      setPatternUrls([]);
      return;
    }
    let cancelled = false;
    const t = setTimeout(() => {
      api
        .expandBatchPattern(p)
        .then((urls) => {
          if (cancelled) return;
          setPatternUrls(urls);
          setError(null);
        })
        .catch((err) => {
          if (cancelled) return;
          setPatternUrls([]);
          setError((err as AppError).message);
        });
    }, 250);
    return () => {
      cancelled = true;
      clearTimeout(t);
    };
  }, [pattern]);

  const listUrls = useMemo(() => linksIn(listText), [listText]);
  const urls = mode === "pattern" ? patternUrls : listUrls;

  async function importFile() {
    const picked = await openPicker({ multiple: false, title: "Import links from a text file", filters: [{ name: "Text files", extensions: ["txt", "csv"] }] });
    if (typeof picked !== "string") return;
    try {
      const found = await api.importUrlList(picked);
      setListText((prev) => (prev.trim() ? `${prev.trim()}\n` : "") + found.join("\n"));
      setError(null);
    } catch (err) {
      setError((err as AppError).message);
    }
  }

  async function browse() {
    const picked = await openPicker({ directory: true, multiple: false, title: "Choose download folder" });
    if (typeof picked === "string") setDestinationDir(picked);
  }

  async function submit(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await onAdd({ urls, destinationDir: destinationDir.trim() || undefined, queueId, start: startNow });
      onClose();
    } catch (err) {
      setError((err as AppError).message);
    } finally {
      setBusy(false);
    }
  }

  return (
    <form onSubmit={submit} noValidate>
      <div className="tabs" role="tablist" aria-label="Batch type">
        <button type="button" role="tab" aria-selected={mode === "pattern"} onClick={() => setMode("pattern")}>
          Numbered Links
        </button>
        <button type="button" role="tab" aria-selected={mode === "list"} onClick={() => setMode("list")}>
          List of Links
        </button>
        <button type="button" className="icon-button tabs-close" aria-label="Close" onClick={onClose}>
          <X size={18} />
        </button>
      </div>
      <div className="modal-body">
        {mode === "pattern" ? (
          <label className="field">
            <span className="field-label">Link pattern</span>
            <input
              type="text"
              value={pattern}
              onChange={(e) => setPattern(e.target.value)}
              placeholder="https://example.com/photos/img[001-120].jpg"
              spellCheck={false}
              autoComplete="off"
              autoFocus
            />
            <span className="hint">Use one range: [1-20], [01-20] keeps leading zeros, [a-z] for letters.</span>
          </label>
        ) : (
          <div className="field">
            <div className="field-label-row">
              <span className="field-label">Links (one per line)</span>
              <button type="button" className="link-button" onClick={importFile}>
                <FileUp size={14} aria-hidden /> Import .txt…
              </button>
            </div>
            <textarea
              className="links-input"
              value={listText}
              onChange={(e) => setListText(e.target.value)}
              placeholder={"https://example.com/a.zip\nhttps://example.com/b.zip"}
              spellCheck={false}
              rows={7}
              autoFocus
            />
          </div>
        )}

        <div className="batch-preview" aria-live="polite">
          {urls.length === 0 ? (
            <span className="muted">No links yet.</span>
          ) : (
            <>
              <strong>{urls.length} {urls.length === 1 ? "link" : "links"}</strong>
              <ul>
                {urls.slice(0, 3).map((u) => (
                  <li key={u}>{u}</li>
                ))}
                {urls.length > 4 && <li className="muted">…</li>}
                {urls.length > 3 && <li>{urls[urls.length - 1]}</li>}
              </ul>
            </>
          )}
        </div>

        <div className="field-pair">
          <label className="field">
            <span className="field-label">Queue</span>
            <select value={queueId} onChange={(e) => setQueueId(e.target.value)}>
              {queues.map((q) => (
                <option key={q.id} value={q.id}>
                  {q.name}
                </option>
              ))}
            </select>
          </label>
          <div className="field grow">
            <span className="field-label" id="batch-dest">Save to</span>
            <div className="input-row">
              <input type="text" aria-labelledby="batch-dest" value={destinationDir} onChange={(e) => setDestinationDir(e.target.value)} placeholder="Automatic (by file type)" spellCheck={false} />
              <button type="button" className="icon-button bordered" onClick={browse} aria-label="Browse" title="Browse">
                <FolderOpen size={16} />
              </button>
            </div>
          </div>
        </div>

        <label className="check">
          <input type="checkbox" checked={startNow} onChange={(e) => setStartNow(e.target.checked)} />
          Start when the queue runs
        </label>

        {error && (
          <p className="form-error" role="alert">
            {error}
          </p>
        )}
      </div>
      <footer className="modal-footer">
        <button type="button" className="button" onClick={onClose}>
          Cancel
        </button>
        <button type="submit" className="button primary" disabled={busy || urls.length === 0}>
          {busy ? "Adding…" : `Add ${urls.length || ""} ${urls.length === 1 ? "Download" : "Downloads"}`.replace("  ", " ")}
        </button>
      </footer>
    </form>
  );
}
