import { useState, type FormEvent } from "react";
import { Modal } from "./Modal";
import { api } from "../lib/api";
import { formatBytes } from "../lib/format";
import type { AppError, Download } from "../lib/types";

interface RefreshLinkDialogProps {
  download: Download | null;
  onClose: () => void;
  onDone: (d: Download) => void;
}

/** "Refresh Download Link": continue an unfinished download from a new link to the same file. */
export function RefreshLinkDialog({ download, onClose, onDone }: RefreshLinkDialogProps) {
  return (
    <Modal open={download !== null} title="Refresh download link" onClose={onClose} className="confirm-dialog">
      {download && <RefreshForm download={download} onClose={onClose} onDone={onDone} />}
    </Modal>
  );
}

function RefreshForm({ download, onClose, onDone }: { download: Download } & Omit<RefreshLinkDialogProps, "download">) {
  const [url, setUrl] = useState("");
  const [resume, setResume] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const progress = download.totalBytes ? `${formatBytes(download.downloadedBytes)} of ${formatBytes(download.totalBytes)}` : formatBytes(download.downloadedBytes);

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (!url.trim()) return;
    setBusy(true);
    setError(null);
    try {
      onDone(await api.refreshDownloadLink(download.id, url.trim(), resume));
      onClose();
    } catch (err) {
      setError((err as AppError).message);
      setBusy(false);
    }
  }

  return (
    <form onSubmit={submit} noValidate>
      <div className="modal-body">
        <h2>Refresh Download Link</h2>
        <p>
          If the link stopped working (it expired, or the site gives a new one each visit), paste a fresh link to the same file.
          BDM keeps the {progress} already downloaded and checks the new link is for the same file.
        </p>
        <label className="field">
          <span className="field-label">{download.fileName}</span>
          <input type="url" value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://…" spellCheck={false} autoFocus />
        </label>
        <label className="check small">
          <input type="checkbox" checked={resume} onChange={(e) => setResume(e.target.checked)} />
          Resume right away
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
        <button type="submit" className="button primary" disabled={busy || !url.trim()}>
          {busy ? "Checking…" : "Use This Link"}
        </button>
      </footer>
    </form>
  );
}
