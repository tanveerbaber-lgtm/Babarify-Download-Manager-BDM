import { useEffect, useMemo, useState, type FormEvent } from "react";
import { open as openFolderPicker } from "@tauri-apps/plugin-dialog";
import { FileArchive, FileAudio, FileText, FileVideo, File, FolderOpen, HardDrive, History, Package, RefreshCw, X, type LucideIcon } from "lucide-react";
import { Modal } from "./Modal";
import { api } from "../lib/api";
import { formatBytes, formatDate } from "../lib/format";
import type { AppError, BrowserProposal, Category, Download, Queue, Settings } from "../lib/types";

interface DownloadInfoDialogProps {
  proposal: BrowserProposal;
  settings: Settings;
  categories: Category[];
  queues: Queue[];
  onClose: () => void;
  /** Called with what was added (one item) so the caller can update the list. */
  onAdded: (created: Download[], started: boolean) => void;
  onCategoriesChanged: () => void;
}

/** "New Download": confirm or adjust a download the browser sent. */
export function DownloadInfoDialog(props: DownloadInfoDialogProps) {
  return (
    <Modal open title="New download" onClose={props.onClose} className="info-dialog">
      <InfoForm {...props} />
    </Modal>
  );
}

function folderFor(category: Category | undefined, defaultDir: string): string {
  const base = defaultDir.replace(/[\\/]+$/, "");
  return category ? (category.folder ?? `${base}\\${category.name}`) : base;
}

function splitName(name: string): [string, string] {
  const i = name.lastIndexOf(".");
  return i > 0 ? [name.slice(0, i), name.slice(i)] : [name, ""];
}

function InfoForm({ proposal, settings, categories, queues, onClose, onAdded, onCategoriesChanged }: DownloadInfoDialogProps) {
  const isMedia = proposal.kind === "media";
  const [categoryId, setCategoryId] = useState(proposal.category);
  const category = categories.find((c) => c.id === categoryId);
  const automaticFolder = settings.sortIntoCategories || categoryId !== proposal.category ? folderFor(category, settings.defaultDownloadDir) : settings.defaultDownloadDir;
  const [folderEdit, setFolderEdit] = useState<string | null>(null);
  const folder = folderEdit ?? automaticFolder;
  // Media names get their real extension from yt-dlp, so only the stem is editable there.
  const [stem, ext] = useMemo(() => splitName(proposal.fileName), [proposal.fileName]);
  const [name, setName] = useState(isMedia ? stem : proposal.fileName);
  const [remember, setRemember] = useState(false);
  const [queueId, setQueueId] = useState("main");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [free, setFree] = useState<number | null>(null);

  // Free space where it will be saved, checked again when the folder changes.
  useEffect(() => {
    let cancelled = false;
    const t = setTimeout(() => {
      api
        .diskFreeSpace(folder)
        .then((n) => !cancelled && setFree(n))
        .catch(() => !cancelled && setFree(null));
    }, 250);
    return () => {
      cancelled = true;
      clearTimeout(t);
    };
  }, [folder]);
  const tooBig = proposal.size != null && free != null && proposal.size + DISK_RESERVE > free;

  const icons: Record<string, LucideIcon> = { videos: FileVideo, music: FileAudio, documents: FileText, compressed: FileArchive, programs: Package };
  const Icon = isMedia ? (proposal.options?.mode === "audio" ? FileAudio : FileVideo) : (icons[categoryId] ?? File);

  async function browse() {
    const picked = await openFolderPicker({ directory: true, multiple: false, defaultPath: folder, title: "Choose where to save" });
    if (typeof picked === "string") setFolderEdit(picked);
  }

  async function submit(start: boolean) {
    setBusy(true);
    setError(null);
    try {
      if (remember && category && folderEdit) {
        await api.saveCategory(category.name, category.extensions, folderEdit, category.id);
        onCategoriesChanged();
      }
      // BDM creates the folder if it doesn't exist yet (e.g. a new category folder).
      const destinationDir = folder.trim() || undefined;
      let created: Download[];
      if (isMedia && proposal.options) {
        created = await api.addMediaDownloads({
          items: [{ url: proposal.url, title: name.trim() || proposal.title || undefined, referrer: proposal.referrer ?? undefined }],
          options: proposal.options,
          destinationDir,
          start,
          category: categoryId,
        });
      } else {
        created = [
          await api.addDownload({
            url: proposal.url,
            fileName: name.trim() || undefined,
            destinationDir,
            referrer: proposal.referrer ?? undefined,
            category: categoryId,
            queueId,
            start,
          }),
        ];
      }
      onAdded(created, start);
      onClose();
    } catch (err) {
      setError((err as AppError).message);
      setBusy(false);
    }
  }

  function onSubmit(e: FormEvent) {
    e.preventDefault();
    void submit(true);
  }

  /** Continue the matching unfinished download with this link instead of starting over. */
  async function continueCandidate(candidate: Download) {
    setBusy(true);
    setError(null);
    try {
      await api.refreshDownloadLink(candidate.id, proposal.url, true);
      onClose();
    } catch (err) {
      setError((err as AppError).message);
      setBusy(false);
    }
  }

  const categoryName = category?.name ?? "Other";
  return (
    <form onSubmit={onSubmit} noValidate>
      <header className="modal-header">
        <h2>New Download</h2>
        <button type="button" className="icon-button" aria-label="Close" onClick={onClose}>
          <X size={18} />
        </button>
      </header>
      <div className="modal-body">
        <div className="info-head">
          <div className="info-icon">
            <Icon size={30} aria-hidden />
          </div>
          <div className="info-meta">
            <div className="info-title">{proposal.title ?? proposal.fileName}</div>
            <div className="info-sub">
              {proposal.label && <span className="info-chip">{proposal.label}</span>}
              <span>{proposal.size ? (isMedia ? `≈ ${formatBytes(proposal.size)}` : formatBytes(proposal.size)) : "Size unknown"}</span>
              <span className="info-url" title={proposal.url}>
                {hostOf(proposal.url)}
              </span>
            </div>
          </div>
        </div>

        {proposal.duplicate && (
          <p className="notice duplicate">
            <History size={15} aria-hidden />
            <span>
              Already in your list ({proposal.duplicate.status === "completed" ? `downloaded ${formatDate(proposal.duplicate.completedAt ?? proposal.duplicate.createdAt)}` : proposal.duplicate.status}). Downloading again saves a new copy.
            </span>
          </p>
        )}

        {proposal.resumeCandidate && (
          <div className="notice resume">
            <RefreshCw size={15} aria-hidden />
            <span>
              Looks like your unfinished download of this file ({resumeProgress(proposal.resumeCandidate)}). Continue it with this new link
              instead of starting again?
            </span>
            <button type="button" className="button small" disabled={busy} onClick={() => continueCandidate(proposal.resumeCandidate as Download)}>
              Continue It
            </button>
          </div>
        )}

        {tooBig && (
          <p className="notice duplicate">
            <HardDrive size={15} aria-hidden />
            <span>
              Not enough space on this drive: {isMedia ? "it's about" : "it's"} {formatBytes(proposal.size ?? 0)} and {formatBytes(free ?? 0)} is free. Choose another
              folder, or free up space first.
            </span>
          </p>
        )}

        <label className="field">
          <span className="field-label">File name</span>
          <div className="input-row">
            <input type="text" value={name} onChange={(e) => setName(e.target.value)} spellCheck={false} autoFocus />
            {isMedia && <span className="ext-tag">{ext}</span>}
          </div>
        </label>

        <div className="field-pair">
          <label className="field grow">
            <span className="field-label">Category</span>
            <select
              value={categoryId}
              onChange={(e) => {
                setCategoryId(e.target.value);
                setFolderEdit(null);
              }}
            >
              {categories.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name}
                </option>
              ))}
              <option value="other">Other</option>
            </select>
          </label>
          {!isMedia && (
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
          )}
        </div>

        <div className="field">
          <span className="field-label" id="info-folder">Save to</span>
          <div className="input-row">
            <input type="text" aria-labelledby="info-folder" value={folder} onChange={(e) => setFolderEdit(e.target.value)} spellCheck={false} />
            <button type="button" className="button" onClick={browse}>
              <FolderOpen size={16} aria-hidden />
              Browse
            </button>
          </div>
          {category && folderEdit && folderEdit !== automaticFolder && (
            <label className="check small">
              <input type="checkbox" checked={remember} onChange={(e) => setRemember(e.target.checked)} />
              Always save {categoryName} here
            </label>
          )}
        </div>

        {error && (
          <p className="form-error" role="alert">
            {error}
          </p>
        )}
      </div>
      <footer className="modal-footer">
        <button type="button" className="button footer-left" onClick={onClose}>
          Cancel
        </button>
        <button type="button" className="button" disabled={busy} onClick={() => submit(false)} title="Add it to the list without starting">
          Download Later
        </button>
        <button type="submit" className="button primary" disabled={busy}>
          Start Download
        </button>
      </footer>
    </form>
  );
}

/** Space BDM always leaves free (engine/disk.rs DEFAULT_RESERVE). */
const DISK_RESERVE = 256 * 1024 * 1024;

function resumeProgress(d: Download): string {
  const pct = d.totalBytes ? Math.floor((d.downloadedBytes / d.totalBytes) * 100) : null;
  return pct === null ? `${formatBytes(d.downloadedBytes)} so far` : `${pct}% of ${formatBytes(d.totalBytes ?? 0)}`;
}

function hostOf(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}
