import { RefreshCw, Download as DownloadIcon } from "lucide-react";
import { ToolProgressBar } from "./MediaToolsSetup";
import { useMediaTools } from "../hooks/useMediaTools";
import type { Settings, SettingsPatch, ToolSource } from "../lib/types";

const SOURCE_LABEL: Record<ToolSource, string> = {
  managed: "Managed by BDM",
  system: "System copy",
  missing: "Not installed",
};

interface MediaToolsCardProps {
  settings: Settings;
  onUpdate: (patch: SettingsPatch) => Promise<boolean>;
}

export function MediaToolsCard({ settings, onUpdate }: MediaToolsCardProps) {
  const { tools, installing, progress, outdated, error, install, checkUpdates } = useMediaTools();
  const anyMissing = tools?.some((t) => t.source === "missing") ?? false;

  return (
    <section className="card">
      <h2>Media tools</h2>
      {!tools && <p className="hint">Checking tools…</p>}
      {tools?.map((t) => (
        <div key={t.kind} className="setting-row">
          <div>
            <div className="setting-title">
              {t.name} {t.version && <span className="muted">{t.version}</span>}
              {outdated?.includes(t.kind) && <span className="badge badge-pending tool-badge">Update available</span>}
            </div>
            <div className="setting-desc">{t.purpose}</div>
            {t.path && <div className="setting-desc path">{t.path}</div>}
          </div>
          <span className={`badge ${t.source === "missing" ? (t.required ? "badge-failed" : "badge-cancelled") : t.source === "managed" ? "badge-completed" : "badge-queued"}`}>
            {SOURCE_LABEL[t.source]}
          </span>
        </div>
      ))}

      {installing && progress && tools && <ToolProgressBar progress={progress} tools={tools} />}
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
      {outdated && outdated.length === 0 && <p className="hint">BDM's tools are up to date.</p>}

      <div className="setting-row">
        <label htmlFor="auto-update-ytdlp">
          <div className="setting-title">Keep yt-dlp up to date</div>
          <div className="setting-desc">Sites change often; BDM checks for a new yt-dlp at most once a day.</div>
        </label>
        <input
          id="auto-update-ytdlp"
          type="checkbox"
          className="switch"
          checked={settings.autoUpdateYtDlp}
          onChange={(e) => void onUpdate({ autoUpdateYtDlp: e.target.checked })}
        />
      </div>

      <div className="card-actions">
        <button type="button" className="button" onClick={() => void checkUpdates()} disabled={installing}>
          <RefreshCw size={15} aria-hidden />
          Check for updates
        </button>
        <button type="button" className="button primary" onClick={() => void install()} disabled={installing}>
          <DownloadIcon size={15} aria-hidden />
          {installing ? "Installing…" : anyMissing ? "Install tools" : "Install / update BDM's copies"}
        </button>
      </div>
      <p className="hint">Downloads come from the official GitHub releases of yt-dlp, FFmpeg (BtbN builds) and Deno, and are checked against their published SHA-256 before use.</p>
    </section>
  );
}
