import { useEffect, useState } from "react";
import { open as openFolderPicker } from "@tauri-apps/plugin-dialog";
import { FolderOpen, Monitor, Moon, Sun, type LucideIcon } from "lucide-react";
import { MediaToolsCard } from "./MediaToolsCard";
import { api } from "../lib/api";
import type { AppError, AppInfo, Settings, SettingsPatch, Theme } from "../lib/types";

const THEMES: { value: Theme; label: string; icon: LucideIcon }[] = [
  { value: "system", label: "System", icon: Monitor },
  { value: "light", label: "Light", icon: Sun },
  { value: "dark", label: "Dark", icon: Moon },
];

interface SettingsViewProps {
  settings: Settings;
  onUpdate: (patch: SettingsPatch) => Promise<Settings>;
}

export function SettingsView({ settings, onUpdate }: SettingsViewProps) {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [concurrent, setConcurrent] = useState(String(settings.maxConcurrentDownloads));
  const [segments, setSegments] = useState(String(settings.segmentsPerDownload));

  useEffect(() => {
    api.getAppInfo().then(setInfo).catch(() => setInfo(null));
  }, []);

  useEffect(() => {
    setConcurrent(String(settings.maxConcurrentDownloads));
  }, [settings.maxConcurrentDownloads]);

  useEffect(() => {
    setSegments(String(settings.segmentsPerDownload));
  }, [settings.segmentsPerDownload]);

  async function save(patch: SettingsPatch): Promise<boolean> {
    setError(null);
    try {
      await onUpdate(patch);
      return true;
    } catch (err) {
      setError((err as AppError).message);
      return false;
    }
  }

  async function browse() {
    const picked = await openFolderPicker({
      directory: true,
      multiple: false,
      defaultPath: settings.defaultDownloadDir,
      title: "Choose default download folder",
    });
    if (typeof picked === "string") await save({ defaultDownloadDir: picked });
  }

  /** Saves a numeric field, showing the saved value again if it's rejected. */
  async function commitNumber(
    key: "maxConcurrentDownloads" | "segmentsPerDownload",
    draft: string,
    setDraft: (v: string) => void,
  ) {
    const saved = String(settings[key]);
    const n = Number(draft);
    if (!Number.isInteger(n) || n === settings[key]) {
      setDraft(saved);
      return;
    }
    if (!(await save({ [key]: n }))) setDraft(saved);
  }

  const commitConcurrent = () => commitNumber("maxConcurrentDownloads", concurrent, setConcurrent);
  const commitSegments = () => commitNumber("segmentsPerDownload", segments, setSegments);

  return (
    <div className="settings">
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}

      <section className="card">
        <h2>Appearance</h2>
        <div className="setting-row">
          <div>
            <div className="setting-title">Theme</div>
            <div className="setting-desc">Follow Windows or choose a fixed theme.</div>
          </div>
          <div className="segmented" role="radiogroup" aria-label="Theme">
            {THEMES.map(({ value, label, icon: Icon }) => (
              <button
                key={value}
                type="button"
                role="radio"
                aria-checked={settings.theme === value}
                onClick={() => save({ theme: value })}
              >
                <Icon size={15} aria-hidden />
                {label}
              </button>
            ))}
          </div>
        </div>
      </section>

      <section className="card">
        <h2>Downloads</h2>
        <div className="setting-row">
          <div>
            <div className="setting-title">Default folder</div>
            <div className="setting-desc path">{settings.defaultDownloadDir}</div>
          </div>
          <button type="button" className="button" onClick={browse}>
            <FolderOpen size={16} aria-hidden />
            Change…
          </button>
        </div>
        <div className="setting-row">
          <label htmlFor="max-concurrent">
            <div className="setting-title">Simultaneous downloads</div>
            <div className="setting-desc">How many downloads may run at once (1–16).</div>
          </label>
          <input
            id="max-concurrent"
            className="number-input"
            type="number"
            min={1}
            max={16}
            value={concurrent}
            onChange={(e) => setConcurrent(e.target.value)}
            onBlur={commitConcurrent}
            onKeyDown={(e) => e.key === "Enter" && commitConcurrent()}
          />
        </div>
        <div className="setting-row">
          <label htmlFor="segments">
            <div className="setting-title">Connections per download</div>
            <div className="setting-desc">Parallel connections for servers that support it (1–16).</div>
          </label>
          <input
            id="segments"
            className="number-input"
            type="number"
            min={1}
            max={16}
            value={segments}
            onChange={(e) => setSegments(e.target.value)}
            onBlur={commitSegments}
            onKeyDown={(e) => e.key === "Enter" && commitSegments()}
          />
        </div>
        <div className="setting-row">
          <label htmlFor="confirm-remove">
            <div className="setting-title">Confirm before removing</div>
            <div className="setting-desc">Ask before removing downloads from the list.</div>
          </label>
          <input
            id="confirm-remove"
            type="checkbox"
            className="switch"
            checked={settings.confirmBeforeRemove}
            onChange={(e) => save({ confirmBeforeRemove: e.target.checked })}
          />
        </div>
      </section>

      <MediaToolsCard settings={settings} onUpdate={save} />

      <section className="card">
        <h2>About</h2>
        <dl className="about">
          <dt>Version</dt>
          <dd>{info?.version ?? "—"}</dd>
          <dt>Data folder</dt>
          <dd className="path">{info?.dataDir ?? "—"}</dd>
          <dt>Database schema</dt>
          <dd>{info ? `v${info.schemaVersion}` : "—"}</dd>
        </dl>
      </section>
    </div>
  );
}
