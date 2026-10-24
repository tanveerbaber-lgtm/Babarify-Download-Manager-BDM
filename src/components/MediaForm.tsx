import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { open as openFolderPicker } from "@tauri-apps/plugin-dialog";
import { FolderOpen, Link2, Search, ShieldAlert, X } from "lucide-react";
import { MediaToolsSetup } from "./MediaToolsSetup";
import { useMediaTools } from "../hooks/useMediaTools";
import { api } from "../lib/api";
import { formatBytes, formatDuration } from "../lib/format";
import type { AppError, AudioFormat, Container, Download, FormatInfo, MediaInfo, MediaOptions, NewMediaRequest } from "../lib/types";

type Mode = "video" | "audio" | "format";

/** Height caps offered for playlists, where per-video formats aren't known. */
const PLAYLIST_CAPS = [2160, 1440, 1080, 720, 480, 360];

const AUDIO_FORMATS: { value: AudioFormat; label: string }[] = [
  { value: "mp3", label: "MP3" },
  { value: "m4a", label: "M4A (AAC)" },
  { value: "opus", label: "Opus" },
  { value: "best", label: "Original (no conversion)" },
];

interface MediaFormProps {
  defaultDir: string;
  initialUrl: string;
  /** Analyze `initialUrl` as soon as the tools are ready. */
  autoAnalyze?: boolean;
  onClose: () => void;
  onAdd: (request: NewMediaRequest) => Promise<Download[]>;
}

export function MediaForm({ defaultDir, initialUrl, autoAnalyze, onClose, onAdd }: MediaFormProps) {
  const tools = useMediaTools();
  const [url, setUrl] = useState(initialUrl);
  const [info, setInfo] = useState<MediaInfo | null>(null);
  const [analyzing, setAnalyzing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const [mode, setMode] = useState<Mode>("video");
  const [maxHeight, setMaxHeight] = useState<number | null>(null);
  const [container, setContainer] = useState<Container>("mp4");
  const [audioFormat, setAudioFormat] = useState<AudioFormat>("mp3");
  const [formatId, setFormatId] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [destinationDir, setDestinationDir] = useState(defaultDir);
  const [startNow, setStartNow] = useState(true);
  const urlInput = useRef<HTMLInputElement>(null);

  useEffect(() => {
    urlInput.current?.focus();
  }, [tools.ready]);

  const autoStarted = useRef(false);
  useEffect(() => {
    if (autoAnalyze && tools.ready && initialUrl && !autoStarted.current) {
      autoStarted.current = true;
      void analyze();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [autoAnalyze, tools.ready, initialUrl]);

  async function analyze(e?: FormEvent) {
    e?.preventDefault();
    if (!url.trim()) return;
    setAnalyzing(true);
    setError(null);
    setInfo(null);
    try {
      const result = await api.analyzeMedia(url.trim());
      setInfo(result);
      setMode("video");
      setFormatId(null);
      // Default to the best quality up to 1080p: a sensible size for most people.
      const capped = result.qualities.find((q) => q.height <= 1080);
      setMaxHeight(result.kind === "playlist" ? 1080 : (capped?.height ?? null));
      setSelected(new Set(result.entries.map((x) => x.url)));
    } catch (err) {
      setError((err as AppError).message);
    } finally {
      setAnalyzing(false);
    }
  }

  const options: MediaOptions | null = useMemo(() => {
    if (mode === "audio") return { mode: "audio", format: audioFormat };
    if (mode === "format") {
      const f = info?.formats.find((x) => x.id === formatId);
      if (!f) return null;
      // A video-only stream needs the best audio merged in.
      return { mode: "format", formatId: f.content === "video" ? `${f.id}+ba` : f.id, container };
    }
    return { mode: "video", maxHeight, container };
  }, [mode, audioFormat, formatId, container, maxHeight, info]);

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (!info || !options) return;
    const items =
      info.kind === "playlist"
        ? info.entries.filter((x) => selected.has(x.url)).map((x) => ({ url: x.url, title: x.title }))
        : [{ url: info.url, title: info.title }];
    setSubmitting(true);
    setError(null);
    try {
      await onAdd({ items, options, destinationDir: destinationDir.trim() || undefined, start: startNow });
      onClose();
    } catch (err) {
      setError((err as AppError).message);
    } finally {
      setSubmitting(false);
    }
  }

  async function browse() {
    const picked = await openFolderPicker({ directory: true, multiple: false, defaultPath: destinationDir || undefined, title: "Choose download folder" });
    if (typeof picked === "string") setDestinationDir(picked);
  }

  if (!tools.tools) {
    return (
      <>
        <Header onClose={onClose} />
        <div className="modal-body">
          <p className="hint">Checking media tools…</p>
        </div>
      </>
    );
  }

  if (!tools.ready) {
    return (
      <>
        <Header onClose={onClose} />
        <div className="modal-body">
          <MediaToolsSetup
            tools={tools.tools}
            installing={tools.installing}
            progress={tools.progress}
            error={tools.error}
            onInstall={() => void tools.install()}
          />
        </div>
      </>
    );
  }

  const isPlaylist = info?.kind === "playlist";
  const blocked = info?.drmProtected || info?.isLive;
  const count = isPlaylist ? selected.size : 1;
  const canSubmit = !!info && !blocked && !!options && count > 0 && !submitting;

  return (
    <>
      <Header onClose={onClose} />
      <form className="modal-body" onSubmit={analyze}>
        <label className="field">
          <span className="field-label">Video, audio or playlist URL</span>
          <div className="input-row">
            <div className="input-with-icon grow">
              <Link2 size={16} aria-hidden />
              <input
                ref={urlInput}
                type="url"
                placeholder="https://…"
                value={url}
                onChange={(e) => setUrl(e.target.value)}
                spellCheck={false}
                autoComplete="off"
              />
            </div>
            <button type="submit" className="button" disabled={analyzing || !url.trim()}>
              <Search size={16} aria-hidden />
              {analyzing ? "Looking…" : "Find formats"}
            </button>
          </div>
        </label>
        {analyzing && <p className="hint">Asking the site which formats are available…</p>}
      </form>

      {info && (
        <form onSubmit={submit}>
          <div className="modal-body media-body">
            <div className="media-summary">
              <div className="media-title">{info.title}</div>
              <div className="muted">
                {[info.uploader, info.duration ? formatDuration(info.duration) : null, info.site, isPlaylist ? `${info.entries.length} videos` : null]
                  .filter(Boolean)
                  .join(" · ")}
              </div>
              {info.license && <div className="license">License: {info.license}</div>}
            </div>

            {info.drmProtected && (
              <p className="form-error" role="alert">
                <ShieldAlert size={15} aria-hidden /> This media is protected by DRM. BDM doesn't download DRM-protected content.
              </p>
            )}
            {info.isLive && <p className="form-error">Live streams aren't supported yet.</p>}

            {!blocked && (
              <>
                <div className="segmented" role="radiogroup" aria-label="What to download">
                  {(["video", "audio"] as const).map((m) => (
                    <button key={m} type="button" role="radio" aria-checked={mode === m || (m === "video" && mode === "format")} onClick={() => setMode(m)}>
                      {m === "video" ? "Video" : "Audio only"}
                    </button>
                  ))}
                </div>

                {mode !== "audio" ? (
                  <div className="input-row">
                    <label className="field grow">
                      <span className="field-label">Quality</span>
                      <select
                        value={mode === "format" ? "format" : String(maxHeight ?? "best")}
                        onChange={(e) => {
                          if (e.target.value === "format") return;
                          setMode("video");
                          setMaxHeight(e.target.value === "best" ? null : Number(e.target.value));
                        }}
                      >
                        <option value="best">Best available</option>
                        {isPlaylist
                          ? PLAYLIST_CAPS.map((h) => (
                              <option key={h} value={h}>
                                Up to {h}p
                              </option>
                            ))
                          : info.qualities.map((q) => {
                              const size = container === "mp4" ? q.estimatedBytes : q.estimatedBytesBest;
                              return (
                                <option key={q.height} value={q.height}>
                                  {q.label}
                                  {q.fps && q.fps > 30 ? ` ${Math.round(q.fps)}fps` : ""}
                                  {size ? ` · ≈${formatBytes(size)}` : ""}
                                </option>
                              );
                            })}
                        {mode === "format" && <option value="format">Exact format (see Advanced)</option>}
                      </select>
                    </label>
                    <label className="field">
                      <span className="field-label">Container</span>
                      <select value={container} onChange={(e) => setContainer(e.target.value as Container)}>
                        <option value="mp4">MP4 (most compatible)</option>
                        <option value="mkv">MKV (any codec)</option>
                      </select>
                    </label>
                  </div>
                ) : (
                  <label className="field">
                    <span className="field-label">Audio format</span>
                    <select value={audioFormat} onChange={(e) => setAudioFormat(e.target.value as AudioFormat)}>
                      {AUDIO_FORMATS.map((a) => (
                        <option key={a.value} value={a.value}>
                          {a.label}
                        </option>
                      ))}
                    </select>
                    {info.audio?.estimatedBytes != null && <span className="hint">≈{formatBytes(info.audio.estimatedBytes)}</span>}
                  </label>
                )}

                {isPlaylist ? (
                  <PlaylistPicker info={info} selected={selected} onChange={setSelected} />
                ) : (
                  info.formats.length > 0 && (
                    <details className="advanced">
                      <summary>Advanced: choose an exact format</summary>
                      <FormatTable
                        formats={info.formats}
                        selected={mode === "format" ? formatId : null}
                        onSelect={(id) => {
                          setFormatId(id);
                          setMode("format");
                        }}
                      />
                    </details>
                  )
                )}

                <div className="field">
                  <span className="field-label" id="media-dest-label">Save to</span>
                  <div className="input-row">
                    <input type="text" aria-labelledby="media-dest-label" value={destinationDir} onChange={(e) => setDestinationDir(e.target.value)} placeholder="Automatic (Videos or Music folder)" spellCheck={false} />
                    <button type="button" className="button" onClick={browse}>
                      <FolderOpen size={16} aria-hidden />
                      Browse
                    </button>
                  </div>
                </div>

                <label className="check">
                  <input type="checkbox" checked={startNow} onChange={(e) => setStartNow(e.target.checked)} />
                  Start downloading now
                </label>
                <p className="hint">Only download media you have the right to save.</p>
              </>
            )}

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
            <button type="submit" className="button primary" disabled={!canSubmit}>
              {submitting ? "Adding…" : count > 1 ? `Download ${count} videos` : "Download"}
            </button>
          </footer>
        </form>
      )}

      {!info && error && (
        <div className="modal-body">
          <p className="form-error" role="alert">
            {error}
          </p>
        </div>
      )}
    </>
  );
}

function Header({ onClose }: { onClose: () => void }) {
  return (
    <header className="modal-header">
      <h2>Add download</h2>
      <button type="button" className="icon-button" aria-label="Close" onClick={onClose}>
        <X size={18} />
      </button>
    </header>
  );
}

function PlaylistPicker({ info, selected, onChange }: { info: MediaInfo; selected: Set<string>; onChange: (s: Set<string>) => void }) {
  const all = info.entries.length > 0 && selected.size === info.entries.length;
  return (
    <div className="field">
      <div className="playlist-head">
        <span className="field-label">
          Videos ({selected.size} of {info.entries.length} selected)
        </span>
        <button type="button" className="link-button" onClick={() => onChange(all ? new Set() : new Set(info.entries.map((e) => e.url)))}>
          {all ? "Select none" : "Select all"}
        </button>
      </div>
      <ul className="playlist">
        {info.entries.map((e, i) => (
          <li key={e.url}>
            <label className="check">
              <input
                type="checkbox"
                checked={selected.has(e.url)}
                onChange={(ev) => {
                  const next = new Set(selected);
                  if (ev.target.checked) next.add(e.url);
                  else next.delete(e.url);
                  onChange(next);
                }}
              />
              <span className="playlist-index">{i + 1}.</span>
              <span className="playlist-title">{e.title}</span>
              {e.duration != null && <span className="muted">{formatDuration(e.duration)}</span>}
            </label>
          </li>
        ))}
      </ul>
      {info.entriesTruncated && <p className="hint">Only the first {info.entries.length} videos are listed.</p>}
    </div>
  );
}

function describe(f: FormatInfo): string {
  if (f.content === "audio") return `Audio · ${f.acodec ?? f.ext}`;
  const res = f.height ? `${f.height}p${f.fps && f.fps > 30 ? Math.round(f.fps) : ""}` : "Video";
  const codecs = [f.vcodec, f.content === "both" ? f.acodec : null].filter(Boolean).join(" + ");
  return `${res} · ${codecs || f.ext}${f.content === "video" ? " (best audio added)" : ""}`;
}

function FormatTable({ formats, selected, onSelect }: { formats: FormatInfo[]; selected: string | null; onSelect: (id: string) => void }) {
  return (
    <div className="format-table" role="radiogroup" aria-label="Exact format">
      {formats.map((f) => (
        <label key={f.id} className={`format-row${selected === f.id ? " selected" : ""}`}>
          <input type="radio" name="format" checked={selected === f.id} onChange={() => onSelect(f.id)} />
          <span className="format-id">{f.id}</span>
          <span className="format-desc">{describe(f)}</span>
          <span className="format-ext">{f.ext}</span>
          <span className="format-size">{f.sizeBytes ? `${f.sizeIsEstimate ? "≈" : ""}${formatBytes(f.sizeBytes)}` : "—"}</span>
        </label>
      ))}
    </div>
  );
}
