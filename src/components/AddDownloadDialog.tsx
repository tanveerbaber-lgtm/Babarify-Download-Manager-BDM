import { useEffect, useRef, useState, type FormEvent } from "react";
import { open as openFolderPicker } from "@tauri-apps/plugin-dialog";
import { FolderOpen, History, Link2, X } from "lucide-react";
import { Modal } from "./Modal";
import { MediaForm } from "./MediaForm";
import { api } from "../lib/api";
import { formatDate } from "../lib/format";
import type { AppError, Download, NewDownloadRequest, NewMediaRequest, Priority, Queue } from "../lib/types";

/** Sites where a link is usually a page with a video rather than a file. */
const VIDEO_SITES = /(^|\.)(youtube\.com|youtu\.be|vimeo\.com|dailymotion\.com|twitch\.tv|soundcloud\.com|archive\.org|tiktok\.com|bilibili\.com|instagram\.com|facebook\.com|x\.com|twitter\.com)$/i;

function looksLikeVideoPage(raw: string): boolean {
  try {
    return VIDEO_SITES.test(new URL(raw.trim()).hostname);
  } catch {
    return false;
  }
}

interface AddDownloadDialogProps {
  open: boolean;
  defaultDir: string;
  /** When on, an empty folder means "the file type's folder". */
  sortIntoCategories: boolean;
  queues: Queue[];
  onClose: () => void;
  onAdd: (request: NewDownloadRequest) => Promise<Download>;
  onAddMedia: (request: NewMediaRequest) => Promise<Download[]>;
  /** Which tab to show first, and a URL to analyze right away (from the browser). */
  initialTab?: Tab;
  initialMediaUrl?: string;
}

export type Tab = "file" | "media";

export function AddDownloadDialog({ open, defaultDir, sortIntoCategories, queues, onClose, onAdd, onAddMedia, initialTab, initialMediaUrl }: AddDownloadDialogProps) {
  return (
    <Modal open={open} title="Add download" onClose={onClose} className="add-dialog">
      <AddDownloadTabs
        defaultDir={defaultDir}
        sortIntoCategories={sortIntoCategories}
        queues={queues}
        onClose={onClose}
        onAdd={onAdd}
        onAddMedia={onAddMedia}
        initialTab={initialTab}
        initialMediaUrl={initialMediaUrl}
      />
    </Modal>
  );
}

function AddDownloadTabs(props: Omit<AddDownloadDialogProps, "open">) {
  const [tab, setTab] = useState<Tab>(props.initialTab ?? "file");
  const [mediaUrl, setMediaUrl] = useState(props.initialMediaUrl ?? "");
  const [autoAnalyze, setAutoAnalyze] = useState(!!props.initialMediaUrl);
  // Empty = let BDM pick the category folder.
  const initialDir = props.sortIntoCategories ? "" : props.defaultDir;
  return (
    <>
      <div className="tabs" role="tablist" aria-label="Download type">
        <button type="button" role="tab" aria-selected={tab === "file"} onClick={() => setTab("file")}>
          File
        </button>
        <button type="button" role="tab" aria-selected={tab === "media"} onClick={() => setTab("media")}>
          Video / audio
        </button>
        <button type="button" className="icon-button tabs-close" aria-label="Close" onClick={props.onClose}>
          <X size={18} />
        </button>
      </div>
      {tab === "file" ? (
        <AddDownloadForm
          defaultDir={initialDir}
          queues={props.queues}
          onClose={props.onClose}
          onAdd={props.onAdd}
          onSwitchToMedia={(url) => {
            setMediaUrl(url);
            setAutoAnalyze(true);
            setTab("media");
          }}
        />
      ) : (
        <MediaForm
          defaultDir={initialDir}
          initialUrl={mediaUrl}
          autoAnalyze={autoAnalyze}
          onClose={props.onClose}
          onAdd={props.onAddMedia}
        />
      )}
    </>
  );
}

interface AddDownloadFormProps {
  defaultDir: string;
  queues: Queue[];
  onClose: () => void;
  onAdd: (request: NewDownloadRequest) => Promise<Download>;
  onSwitchToMedia: (url: string) => void;
}

function AddDownloadForm({ defaultDir, queues, onClose, onAdd, onSwitchToMedia }: AddDownloadFormProps) {
  const [url, setUrl] = useState("");
  const [fileName, setFileName] = useState("");
  const [fileNameEdited, setFileNameEdited] = useState(false);
  const [destinationDir, setDestinationDir] = useState(defaultDir);
  const [sha256, setSha256] = useState("");
  const [startNow, setStartNow] = useState(true);
  const [queueId, setQueueId] = useState("main");
  const [priority, setPriority] = useState<Priority>(0);
  const [speedLimit, setSpeedLimit] = useState("");
  const [duplicate, setDuplicate] = useState<Download | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const urlInput = useRef<HTMLInputElement>(null);

  useEffect(() => {
    urlInput.current?.focus();
  }, []);

  // Suggest a filename from the URL until the user types their own.
  useEffect(() => {
    if (fileNameEdited) return;
    const trimmed = url.trim();
    if (!trimmed) {
      setFileName("");
      return;
    }
    let cancelled = false;
    const timer = setTimeout(() => {
      api
        .suggestFileName(trimmed)
        .then((name) => !cancelled && setFileName(name))
        .catch(() => !cancelled && setFileName(""));
    }, 200);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [url, fileNameEdited]);

  // Warn when this link was downloaded before.
  useEffect(() => {
    const trimmed = url.trim();
    setDuplicate(null);
    if (!/^https?:\/\//i.test(trimmed)) return;
    let cancelled = false;
    const timer = setTimeout(() => {
      api
        .findDuplicate(trimmed)
        .then((d) => !cancelled && setDuplicate(d))
        .catch(() => {});
    }, 300);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [url]);

  async function browse() {
    const picked = await openFolderPicker({
      directory: true,
      multiple: false,
      defaultPath: destinationDir || undefined,
      title: "Choose download folder",
    });
    if (typeof picked === "string") setDestinationDir(picked);
  }

  async function submit(e: FormEvent) {
    e.preventDefault();
    setSubmitting(true);
    setError(null);
    try {
      await onAdd({
        url,
        fileName: fileName.trim() || undefined,
        destinationDir: destinationDir.trim() || undefined,
        expectedSha256: sha256.trim() || undefined,
        start: startNow,
        queueId,
        priority,
        speedLimitKbps: Number(speedLimit) > 0 ? Number(speedLimit) : undefined,
      });
      onClose();
    } catch (err) {
      setError((err as AppError).message);
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <form onSubmit={submit} noValidate>
      <header className="modal-header">
        <h2>Add download</h2>
        <button type="button" className="icon-button" aria-label="Close" onClick={onClose}>
          <X size={18} />
        </button>
      </header>

      <div className="modal-body">
        <label className="field">
          <span className="field-label">URL</span>
          <div className="input-with-icon">
            <Link2 size={16} aria-hidden />
            <input
              ref={urlInput}
              type="url"
              inputMode="url"
              placeholder="https://example.com/file.zip"
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              spellCheck={false}
              autoComplete="off"
              required
            />
          </div>
        </label>

        {duplicate && (
          <p className="notice duplicate">
            <History size={15} aria-hidden />
            <span>
              Already in your list: <strong>{duplicate.fileName}</strong> ({duplicate.status === "completed" ? `downloaded ${formatDate(duplicate.completedAt ?? duplicate.createdAt)}` : duplicate.status}). Adding it again saves a new copy.
            </span>
          </p>
        )}

        {looksLikeVideoPage(url) && (
          <p className="notice">
            This looks like a video page.{" "}
            <button type="button" className="link-button" onClick={() => onSwitchToMedia(url.trim())}>
              Download the video instead
            </button>
          </p>
        )}

        <label className="field">
          <span className="field-label">File name</span>
          <input
            type="text"
            placeholder="Detected from the URL"
            value={fileName}
            onChange={(e) => {
              setFileName(e.target.value);
              setFileNameEdited(e.target.value.trim() !== "");
            }}
            spellCheck={false}
            autoComplete="off"
          />
        </label>

        <div className="field">
          <span className="field-label" id="dest-label">Save to</span>
          <div className="input-row">
            <input
              type="text"
              aria-labelledby="dest-label"
              value={destinationDir}
              onChange={(e) => setDestinationDir(e.target.value)}
              placeholder="Automatic (by file type, e.g. Downloads\Videos)"
              spellCheck={false}
            />
            <button type="button" className="button" onClick={browse}>
              <FolderOpen size={16} aria-hidden />
              Browse
            </button>
          </div>
        </div>

        <div className="field-pair">
          <label className="field grow">
            <span className="field-label">Queue</span>
            <select value={queueId} onChange={(e) => setQueueId(e.target.value)}>
              {queues.map((q) => (
                <option key={q.id} value={q.id}>
                  {q.name}
                  {q.schedule.enabled ? " (scheduled)" : q.running ? "" : " (stopped)"}
                </option>
              ))}
            </select>
          </label>
          <label className="field grow">
            <span className="field-label">Priority</span>
            <select value={priority} onChange={(e) => setPriority(Number(e.target.value) as Priority)}>
              <option value={1}>High</option>
              <option value={0}>Normal</option>
              <option value={-1}>Low</option>
            </select>
          </label>
        </div>

        <details className="advanced">
          <summary>Advanced</summary>
          <label className="field">
            <span className="field-label">Speed limit for this download (KB/s, optional)</span>
            <input
              type="text"
              inputMode="numeric"
              placeholder="No limit"
              value={speedLimit}
              onChange={(e) => setSpeedLimit(e.target.value.replace(/\D/g, ""))}
            />
          </label>
          <label className="field">
            <span className="field-label">SHA-256 checksum (optional)</span>
            <input
              type="text"
              placeholder="64 hexadecimal characters"
              value={sha256}
              onChange={(e) => setSha256(e.target.value)}
              spellCheck={false}
              autoComplete="off"
            />
            <span className="hint">The file is only saved if its checksum matches.</span>
          </label>
        </details>

        <label className="check">
          <input type="checkbox" checked={startNow} onChange={(e) => setStartNow(e.target.checked)} />
          Start downloading now
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
        <button type="submit" className="button primary" disabled={submitting || !url.trim()}>
          {submitting ? "Adding…" : startNow ? "Download" : "Add to list"}
        </button>
      </footer>
    </form>
  );
}
