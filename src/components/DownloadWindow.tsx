import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { CheckCircle2, ChevronDown, ChevronUp, Copy, FolderOpen, Gauge, Pause, Play, Square, X } from "lucide-react";
import { api, EVENTS } from "../lib/api";
import { useSettings } from "../hooks/useSettings";
import { formatBytes, formatEta, formatSpeed, progressPercent, STATUS_LABELS } from "../lib/format";
import { canPause, canResume } from "../lib/actions";
import type { AppError, Download, Progress, WhenDone } from "../lib/types";

type Tab = "status" | "limit" | "done";

interface DoneOptions {
  summary: boolean;
  openFile: boolean;
  openFolder: boolean;
  closeWindow: boolean;
}

const DEFAULT_DONE: DoneOptions = { summary: true, openFile: false, openFolder: false, closeWindow: false };
const DONE_KEY = "bdm.window.done";
const PRESETS = [256, 512, 1024, 2048, 5120];

function loadDone(): DoneOptions {
  try {
    return { ...DEFAULT_DONE, ...JSON.parse(localStorage.getItem(DONE_KEY) ?? "{}") };
  } catch {
    return DEFAULT_DONE;
  }
}

/** The small window for one download: live status, connections, speed limit, completion options. */
export function DownloadWindow({ id }: { id: string }) {
  const { settings, update } = useSettings();
  const [download, setDownload] = useState<Download | null>(null);
  const [progress, setProgress] = useState<Progress | null>(null);
  const [tab, setTab] = useState<Tab>("status");
  const [details, setDetails] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<DoneOptions>(loadDone);
  const [limitOn, setLimitOn] = useState(false);
  const [limit, setLimit] = useState("1024");
  const handledCompletion = useRef(false);

  const refresh = useCallback(async () => {
    const d = (await api.listDownloads()).find((x) => x.id === id) ?? null;
    setDownload(d);
    if (d) {
      setLimitOn(d.speedLimit != null);
      if (d.speedLimit) setLimit(String(Math.round(d.speedLimit / 1024)));
    }
  }, [id]);

  useEffect(() => {
    void refresh();
    const stops = [
      listen<Download>(EVENTS.updated, ({ payload }) => payload.id === id && setDownload(payload)),
      listen<Progress>(EVENTS.progress, ({ payload }) => payload.id === id && setProgress(payload)),
      listen<string>(EVENTS.removed, ({ payload }) => payload === id && void getCurrentWindow().close()),
    ];
    return () => {
      for (const s of stops) void s.then((stop) => stop());
    };
  }, [id, refresh]);

  const status = download?.status;
  const live = status === "downloading" ? progress : null;
  const downloaded = live?.downloadedBytes ?? download?.downloadedBytes ?? 0;
  const total = live?.totalBytes ?? download?.totalBytes ?? null;
  const pct = status === "completed" ? 100 : progressPercent(downloaded, total);
  const name = download ? (download.title ?? download.fileName) : "Download";

  // "37% name.mp4" in the title bar, like the taskbar shows it.
  useEffect(() => {
    if (!download) return;
    const prefix = status === "downloading" && pct != null ? `${Math.floor(pct)}% ` : status === "completed" ? "✓ " : "";
    void getCurrentWindow().setTitle(prefix + name).catch(() => {});
  }, [download, status, pct, name]);

  // Completion actions run once.
  useEffect(() => {
    if (status !== "completed" || handledCompletion.current || !download) return;
    handledCompletion.current = true;
    if (done.openFile) void api.openDownload(download.id).catch(() => {});
    if (done.openFolder) void api.showInFolder(download.id).catch(() => {});
    if (done.closeWindow && !done.summary) void getCurrentWindow().close();
  }, [status, download, done]);

  function setDoneOption(key: keyof DoneOptions, value: boolean) {
    const next = { ...done, [key]: value };
    setDone(next);
    try {
      localStorage.setItem(DONE_KEY, JSON.stringify(next));
    } catch {
      // Only a convenience.
    }
  }

  async function act(fn: () => Promise<unknown>) {
    try {
      setError(null);
      await fn();
    } catch (err) {
      setError((err as AppError).message);
    }
  }

  async function applyLimit(on: boolean, value = limit) {
    setLimitOn(on);
    const kbps = Number(value);
    if (on && !(Number.isInteger(kbps) && kbps > 0)) return;
    await act(() => api.setSpeedLimit([id], on ? kbps : null));
  }

  if (!download) {
    return <div className="dlw dlw-empty">{error ?? "Loading…"}</div>;
  }

  if (status === "completed" && done.summary) {
    return (
      <div className="dlw">
        <div className="dlw-complete">
          <CheckCircle2 size={44} className="dlw-complete-icon" aria-hidden />
          <h1>Download Complete</h1>
          <p className="dlw-name">{name}</p>
          <dl className="dlw-grid">
            <dt>Size</dt>
            <dd>{formatBytes(download.totalBytes)}</dd>
            <dt>Saved to</dt>
            <dd className="path">{download.savedPath}</dd>
          </dl>
          {error && <p className="form-error">{error}</p>}
          <div className="dlw-actions">
            <button type="button" className="button primary" onClick={() => act(() => api.openDownload(id))}>
              Open
            </button>
            <button type="button" className="button" onClick={() => act(() => api.showInFolder(id))}>
              <FolderOpen size={15} aria-hidden />
              Open Folder
            </button>
            <button type="button" className="button" onClick={() => void getCurrentWindow().close()}>
              Close
            </button>
          </div>
        </div>
      </div>
    );
  }

  const segments = live?.segments ?? [];
  const segTotal = total ?? segments.reduce((s, x) => Math.max(s, x.start + x.len), 0);
  const stage = live?.stage ?? (status === "queued" ? "Waiting for a free slot or its queue" : null);

  return (
    <div className="dlw">
      <div className="tabs dlw-tabs" role="tablist">
        <button type="button" role="tab" aria-selected={tab === "status"} onClick={() => setTab("status")}>
          Download Status
        </button>
        <button type="button" role="tab" aria-selected={tab === "limit"} onClick={() => setTab("limit")}>
          Speed Limit
        </button>
        <button type="button" role="tab" aria-selected={tab === "done"} onClick={() => setTab("done")}>
          When Done
        </button>
      </div>

      <section className="dlw-panel">
        {tab === "status" && (
          <dl className="dlw-grid">
            <dt>Address</dt>
            <dd className="dlw-url">
              <span title={download.url}>{download.url}</span>
              <button type="button" className="icon-button small" aria-label="Copy link" title="Copy link" onClick={() => void navigator.clipboard.writeText(download.url)}>
                <Copy size={13} />
              </button>
            </dd>
            <dt>Status</dt>
            <dd className={`dlw-status dlw-status-${status}`}>
              {status === "downloading" ? (stage ?? "Receiving data…") : STATUS_LABELS[status!]}
              {download.errorMessage && status !== "downloading" && <span className="dlw-error">{download.errorMessage}</span>}
            </dd>
            <dt>File size</dt>
            <dd>{formatBytes(total)}</dd>
            <dt>Downloaded</dt>
            <dd>
              {formatBytes(downloaded)}
              {pct != null && ` (${pct.toFixed(1)}%)`}
            </dd>
            <dt>Transfer rate</dt>
            <dd>{formatSpeed(live?.speed)}</dd>
            <dt>Time left</dt>
            <dd>{formatEta(live?.etaSeconds)}</dd>
            <dt>Resume support</dt>
            <dd>{download.kind === "media" ? "Yes (keeps partial streams)" : download.finalUrl ? (download.resumable ? "Yes" : "No — pausing restarts it") : "Checking…"}</dd>
            <dt>Save to</dt>
            <dd className="path">{download.destinationDir}</dd>
          </dl>
        )}

        {tab === "limit" && (
          <div className="dlw-limit">
            <div className="dlw-rate">
              <Gauge size={18} aria-hidden />
              <span>Current speed</span>
              <strong>{formatSpeed(live?.speed)}</strong>
            </div>
            <label className="check">
              <input type="checkbox" checked={limitOn} onChange={(e) => void applyLimit(e.target.checked)} />
              Limit this download's speed
            </label>
            <div className="input-row dlw-limit-row">
              <input
                type="text"
                className="number-input"
                inputMode="numeric"
                disabled={!limitOn}
                value={limit}
                onChange={(e) => setLimit(e.target.value.replace(/\D/g, ""))}
                onBlur={() => limitOn && void applyLimit(true)}
                onKeyDown={(e) => e.key === "Enter" && limitOn && void applyLimit(true)}
              />
              <span className="muted">KB/s</span>
            </div>
            <div className="day-chips">
              {PRESETS.map((p) => (
                <button
                  key={p}
                  type="button"
                  className="chip"
                  aria-pressed={limitOn && Number(limit) === p}
                  onClick={() => {
                    setLimit(String(p));
                    void applyLimit(true, String(p));
                  }}
                >
                  {p >= 1024 ? `${p / 1024} MB/s` : `${p} KB/s`}
                </button>
              ))}
            </div>
            <p className="hint">Applies right away and stays with this download if you pause and resume it.</p>
          </div>
        )}

        {tab === "done" && (
          <div className="dlw-done">
            <label className="check">
              <input type="checkbox" checked={done.summary} onChange={(e) => setDoneOption("summary", e.target.checked)} />
              Show a summary when the download completes
            </label>
            <label className="check">
              <input type="checkbox" checked={done.openFile} onChange={(e) => setDoneOption("openFile", e.target.checked)} />
              Open the file
            </label>
            <label className="check">
              <input type="checkbox" checked={done.openFolder} onChange={(e) => setDoneOption("openFolder", e.target.checked)} />
              Open its folder
            </label>
            <label className="check">
              <input type="checkbox" checked={done.closeWindow} disabled={done.summary} onChange={(e) => setDoneOption("closeWindow", e.target.checked)} />
              Close this window
            </label>
            {settings && (
              <label className="field dlw-when">
                <span className="field-label">When all downloads finish</span>
                <select value={settings.whenAllDone} onChange={(e) => void act(() => update({ whenAllDone: e.target.value as WhenDone }))}>
                  <option value="nothing">Do nothing</option>
                  <option value="sleep">Sleep</option>
                  <option value="shutdown">Shut down (after 60 s)</option>
                  <option value="exit">Close BDM</option>
                </select>
              </label>
            )}
          </div>
        )}
      </section>

      <div className={`progress dlw-progress progress-${status}`} role="progressbar" aria-valuenow={pct ?? undefined} aria-valuemin={0} aria-valuemax={100}>
        <div className="progress-bar" style={{ width: `${pct ?? 0}%` }} />
      </div>

      {error && <p className="form-error">{error}</p>}

      <div className="dlw-actions">
        <button type="button" className="button" onClick={() => setDetails((v) => !v)}>
          {details ? <ChevronUp size={15} aria-hidden /> : <ChevronDown size={15} aria-hidden />}
          {details ? "Hide Details" : "Show Details"}
        </button>
        <span className="dlw-spacer" />
        {canPause(download) && (
          <button type="button" className="button" onClick={() => act(() => api.pauseDownload(id))}>
            <Pause size={15} aria-hidden />
            Pause
          </button>
        )}
        {canResume(download) && (
          <button type="button" className="button primary" onClick={() => act(() => api.resumeDownload(id))}>
            <Play size={15} aria-hidden />
            {status === "pending" ? "Start" : "Resume"}
          </button>
        )}
        {status !== "completed" && status !== "cancelled" && (
          <button type="button" className="button" onClick={() => act(() => api.cancelDownload(id))}>
            <Square size={13} aria-hidden />
            Cancel
          </button>
        )}
        <button type="button" className="icon-button" aria-label="Close window" title="Close window (the download keeps going)" onClick={() => void getCurrentWindow().close()}>
          <X size={17} />
        </button>
      </div>

      {details && (
        <div className="dlw-details">
          <div className="dlw-map-label">Connections and their progress</div>
          <div className="dlw-map" aria-hidden>
            {segTotal > 0 &&
              segments.map((s, i) => (
                <div key={i} className="dlw-seg" style={{ left: `${(s.start / segTotal) * 100}%`, width: `${(s.len / segTotal) * 100}%` }}>
                  <div className="dlw-seg-fill" style={{ width: `${s.len ? (s.done / s.len) * 100 : 100}%` }} />
                </div>
              ))}
            {segments.length === 0 && <div className="dlw-seg-single" style={{ width: `${pct ?? 0}%` }} />}
          </div>
          <table className="dlw-table">
            <thead>
              <tr>
                <th>#</th>
                <th>Downloaded</th>
                <th>Range</th>
                <th>Info</th>
              </tr>
            </thead>
            <tbody>
              {segments.length > 0 ? (
                segments.map((s, i) => (
                  <tr key={i}>
                    <td>{i + 1}</td>
                    <td>{formatBytes(s.done)}</td>
                    <td className="muted">
                      {formatBytes(s.start)} – {formatBytes(s.start + s.len)}
                    </td>
                    <td>{s.done >= s.len ? "Done" : status === "downloading" ? "Receiving data…" : "Waiting"}</td>
                  </tr>
                ))
              ) : (
                <tr>
                  <td>1</td>
                  <td>{formatBytes(downloaded)}</td>
                  <td className="muted">{download.kind === "media" ? "yt-dlp" : "single connection"}</td>
                  <td>{status === "downloading" ? (stage ?? "Receiving data…") : STATUS_LABELS[status!]}</td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
