import { Download as DownloadIcon } from "lucide-react";
import { formatBytes } from "../lib/format";
import type { ToolProgress, ToolStatus } from "../lib/types";

interface MediaToolsSetupProps {
  tools: ToolStatus[];
  installing: boolean;
  progress: ToolProgress | null;
  error: string | null;
  onInstall: () => void;
}

/** Shown in the Add dialog until yt-dlp and FFmpeg are available. */
export function MediaToolsSetup({ tools, installing, progress, error, onInstall }: MediaToolsSetupProps) {
  const missing = tools.filter((t) => t.source === "missing");
  return (
    <div className="setup-card">
      <h3>Set up video downloads</h3>
      <p>
        BDM uses {missing.map((t) => t.name).join(", ")} to download video and audio. It downloads the official releases
        from GitHub and checks their SHA-256 checksums before installing them.
      </p>
      <ul className="tool-list">
        {missing.map((t) => (
          <li key={t.kind}>
            <strong>{t.name}</strong>
            {!t.required && <span className="muted"> (optional)</span>} — {t.purpose}
          </li>
        ))}
      </ul>
      {installing && progress && <ToolProgressBar progress={progress} tools={tools} />}
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
      <button type="button" className="button primary" onClick={onInstall} disabled={installing}>
        <DownloadIcon size={16} aria-hidden />
        {installing ? "Installing…" : "Install media tools"}
      </button>
    </div>
  );
}

export function ToolProgressBar({ progress, tools }: { progress: ToolProgress; tools: ToolStatus[] }) {
  const name = tools.find((t) => t.kind === progress.tool)?.name ?? progress.tool;
  const pct = progress.totalBytes > 0 ? Math.min(100, (progress.downloadedBytes / progress.totalBytes) * 100) : 0;
  return (
    <div className="tool-progress">
      <div className="tool-progress-label">
        <span>
          {progress.stage} {name}
        </span>
        <span className="muted">
          {formatBytes(progress.downloadedBytes)} / {formatBytes(progress.totalBytes)}
        </span>
      </div>
      <div className="progress" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(pct)}>
        <div className="progress-bar" style={{ width: `${pct}%` }} />
      </div>
    </div>
  );
}
