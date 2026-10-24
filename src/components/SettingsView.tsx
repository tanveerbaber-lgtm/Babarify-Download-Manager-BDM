import { useEffect, useState } from "react";
import { open as openFolderPicker } from "@tauri-apps/plugin-dialog";
import { FolderOpen, Monitor, Moon, Pencil, Plus, Sun, type LucideIcon } from "lucide-react";
import { CategoryDialog } from "./CategoryDialog";
import { MediaToolsCard } from "./MediaToolsCard";
import { api } from "../lib/api";
import type { AppError, AppInfo, Category, DuplicateAction, Settings, SettingsPatch, Theme, WhenDone } from "../lib/types";

const THEMES: { value: Theme; label: string; icon: LucideIcon }[] = [
  { value: "system", label: "System", icon: Monitor },
  { value: "light", label: "Light", icon: Sun },
  { value: "dark", label: "Dark", icon: Moon },
];

const WHEN_DONE: { value: WhenDone; label: string }[] = [
  { value: "nothing", label: "Do nothing" },
  { value: "sleep", label: "Sleep" },
  { value: "shutdown", label: "Shut down" },
  { value: "exit", label: "Close BDM" },
];

type NumberKey = "maxConcurrentDownloads" | "segmentsPerDownload" | "speedLimitKbps" | "maxRetries" | "autoRetryCount" | "autoRetryDelayMinutes";

interface SettingsViewProps {
  settings: Settings;
  categories: Category[];
  onUpdate: (patch: SettingsPatch) => Promise<Settings>;
  onCategoriesChanged: () => void;
}

/** A number field that saves on blur or Enter and snaps back if rejected. */
function NumberSetting({
  id,
  title,
  desc,
  value,
  min,
  max,
  onCommit,
}: {
  id: string;
  title: string;
  desc: string;
  value: number;
  min: number;
  max: number;
  onCommit: (n: number) => Promise<boolean>;
}) {
  const [draft, setDraft] = useState(String(value));
  useEffect(() => setDraft(String(value)), [value]);
  async function commit() {
    const n = Number(draft);
    if (!Number.isInteger(n) || n === value) {
      setDraft(String(value));
      return;
    }
    if (!(await onCommit(n))) setDraft(String(value));
  }
  return (
    <div className="setting-row">
      <label htmlFor={id}>
        <div className="setting-title">{title}</div>
        <div className="setting-desc">{desc}</div>
      </label>
      <input
        id={id}
        className="number-input"
        type="number"
        min={min}
        max={max}
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => e.key === "Enter" && commit()}
      />
    </div>
  );
}

function SwitchSetting({ id, title, desc, checked, onChange }: { id: string; title: string; desc: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <div className="setting-row">
      <label htmlFor={id}>
        <div className="setting-title">{title}</div>
        <div className="setting-desc">{desc}</div>
      </label>
      <input id={id} type="checkbox" className="switch" checked={checked} onChange={(e) => onChange(e.target.checked)} />
    </div>
  );
}

export function SettingsView({ settings, categories, onUpdate, onCategoriesChanged }: SettingsViewProps) {
  /** `undefined` = closed, `null` = new category. */
  const [editingCategory, setEditingCategory] = useState<Category | null | undefined>(undefined);
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.getAppInfo().then(setInfo).catch(() => setInfo(null));
  }, []);


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

  const saveNumber = (key: NumberKey) => (n: number) => save({ [key]: n });

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
        <NumberSetting
          id="max-concurrent"
          title="Simultaneous downloads"
          desc="How many downloads may run at once, across all queues (1–16)."
          value={settings.maxConcurrentDownloads}
          min={1}
          max={16}
          onCommit={saveNumber("maxConcurrentDownloads")}
        />
        <NumberSetting
          id="segments"
          title="Connections per download"
          desc="Parallel connections for servers that support it (1–16)."
          value={settings.segmentsPerDownload}
          min={1}
          max={16}
          onCommit={saveNumber("segmentsPerDownload")}
        />
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

      <section className="card">
        <h2>Speed and Retries</h2>
        <NumberSetting
          id="speed-limit"
          title="Global speed limit (KB/s)"
          desc="Shared by every download. 0 means unlimited. Downloads can also have their own limit (right-click a download)."
          value={settings.speedLimitKbps}
          min={0}
          max={10_000_000}
          onCommit={saveNumber("speedLimitKbps")}
        />
        <NumberSetting
          id="max-retries"
          title="Retries per attempt"
          desc="How often a dropped connection, timeout or server error is retried before the download fails (0–10)."
          value={settings.maxRetries}
          min={0}
          max={10}
          onCommit={saveNumber("maxRetries")}
        />
        <NumberSetting
          id="auto-retry"
          title="Automatic retries after a failure"
          desc="Try a failed download again later. Broken links, sign-in pages and checksum failures are never retried (0–10)."
          value={settings.autoRetryCount}
          min={0}
          max={10}
          onCommit={saveNumber("autoRetryCount")}
        />
        <NumberSetting
          id="auto-retry-delay"
          title="Wait before retrying (minutes)"
          desc="How long to wait before each automatic retry (1–1440)."
          value={settings.autoRetryDelayMinutes}
          min={1}
          max={1440}
          onCommit={saveNumber("autoRetryDelayMinutes")}
        />
      </section>

      <section className="card">
        <h2>When Downloads Finish</h2>
        <SwitchSetting
          id="notify-complete"
          title="Notify when a download completes"
          desc="Show a Windows notification for each finished download."
          checked={settings.notifyOnComplete}
          onChange={(v) => save({ notifyOnComplete: v })}
        />
        <SwitchSetting
          id="notify-failure"
          title="Notify when a download fails"
          desc="Only once automatic retries are used up."
          checked={settings.notifyOnFailure}
          onChange={(v) => save({ notifyOnFailure: v })}
        />
        <div className="setting-row">
          <label htmlFor="when-done">
            <div className="setting-title">When all downloads are done</div>
            <div className="setting-desc">Runs once when nothing is left to download, then switches back to “Do nothing”. Shutdown waits 60 seconds.</div>
          </label>
          <select id="when-done" value={settings.whenAllDone} onChange={(e) => save({ whenAllDone: e.target.value as WhenDone })}>
            {WHEN_DONE.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </select>
        </div>
      </section>

      <section className="card">
        <h2>Organize</h2>
        <SwitchSetting
          id="sort-categories"
          title="Sort downloads into category folders"
          desc="Unless you pick a folder, files go to a subfolder by type, e.g. Downloads\Videos."
          checked={settings.sortIntoCategories}
          onChange={(v) => save({ sortIntoCategories: v })}
        />
        <div className="setting-row">
          <label htmlFor="duplicates">
            <div className="setting-title">Links already in the list</div>
            <div className="setting-desc">What to do when the browser sends a link BDM has downloaded before.</div>
          </label>
          <select id="duplicates" value={settings.duplicateAction} onChange={(e) => save({ duplicateAction: e.target.value as DuplicateAction })}>
            <option value="add">Download again</option>
            <option value="skip">Skip it</option>
          </select>
        </div>
        <div className="category-list">
          {categories.map((c) => (
            <div key={c.id} className="category-row">
              <div>
                <div className="setting-title">{c.name}</div>
                <div className="setting-desc">{c.extensions.join(", ")}</div>
                <div className="setting-desc path">{c.folder ?? `${settings.defaultDownloadDir.replace(/[\\/]+$/, "")}\\${c.name}`}</div>
              </div>
              <button type="button" className="icon-button" onClick={() => setEditingCategory(c)} aria-label={`Edit ${c.name}`} title="Edit">
                <Pencil size={15} />
              </button>
            </div>
          ))}
          <button type="button" className="button add-category" onClick={() => setEditingCategory(null)}>
            <Plus size={15} aria-hidden />
            New Category
          </button>
        </div>
      </section>

      <MediaToolsCard settings={settings} onUpdate={save} />

      {editingCategory !== undefined && (
        <CategoryDialog
          category={editingCategory}
          defaultDir={settings.defaultDownloadDir}
          onClose={() => setEditingCategory(undefined)}
          onSaved={onCategoriesChanged}
        />
      )}

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
