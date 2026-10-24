import { useCallback, useEffect, useMemo, useState } from "react";
import { Pause, Play, Plus, Search, Square, Trash2 } from "lucide-react";
import { Sidebar } from "./components/Sidebar";
import { DashboardStats } from "./components/DashboardStats";
import { DownloadTable } from "./components/DownloadTable";
import { AddDownloadDialog } from "./components/AddDownloadDialog";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { SettingsView } from "./components/SettingsView";
import { BrowserView } from "./components/BrowserView";
import { Toasts, type ToastMessage } from "./components/Toast";
import { Logo, TAGLINE } from "./components/Logo";
import type { Tab } from "./components/AddDownloadDialog";
import { listen } from "@tauri-apps/api/event";
import { EVENTS } from "./lib/api";
import { useDownloads, type BulkAction } from "./hooks/useDownloads";
import { useSettings } from "./hooks/useSettings";
import { countByView, inView, LIST_VIEW_TITLES, type View } from "./lib/views";
import { canCancel, canPause, canResume, eligible } from "./lib/actions";
import { formatSpeed } from "./lib/format";
import type { BrowserMedia, Download } from "./lib/types";

export default function App() {
  const { downloads, progress, loading, error: loadError, add, addMedia, run } = useDownloads();
  const { settings, error: settingsError, update } = useSettings();
  const [view, setView] = useState<View>("all");
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [addOpen, setAddOpen] = useState(false);
  const [addTab, setAddTab] = useState<Tab>("file");
  const [addMediaUrl, setAddMediaUrl] = useState<string | undefined>();
  const [toasts, setToasts] = useState<ToastMessage[]>([]);

  const openAdd = useCallback((tab: Tab = "file", mediaUrl?: string) => {
    setAddTab(tab);
    setAddMediaUrl(mediaUrl);
    setAddOpen(true);
  }, []);
  const notify = useCallback((text: string) => setToasts((t) => [...t, { id: Date.now() + Math.random(), text }]), []);
  const dismissToast = useCallback((id: number) => setToasts((t) => t.filter((x) => x.id !== id)), []);

  // Requests relayed from the browser extension.
  useEffect(() => {
    const stops = [
      listen<Download>(EVENTS.browserAdded, ({ payload }) => notify(`Added from your browser: ${payload.fileName}`)),
      listen<BrowserMedia>(EVENTS.browserMedia, ({ payload }) => {
        setView("all");
        setAddOpen(false);
        // Reopen so the dialog starts fresh with the new URL.
        setTimeout(() => openAdd("media", payload.url), 0);
      }),
    ];
    return () => {
      for (const s of stops) void s.then((stop) => stop());
    };
  }, [notify, openAdd]);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);

  const counts = useMemo(() => countByView(downloads), [downloads]);

  const visible = useMemo(() => {
    if (view === "settings" || view === "browser") return [];
    const q = query.trim().toLowerCase();
    return downloads.filter(
      (d) => inView(d, view) && (!q || d.fileName.toLowerCase().includes(q) || d.url.toLowerCase().includes(q)),
    );
  }, [downloads, view, query]);

  // Drop selections that are no longer visible.
  useEffect(() => {
    setSelected((prev) => {
      const ids = new Set(visible.map((d) => d.id));
      const next = new Set([...prev].filter((id) => ids.has(id)));
      return next.size === prev.size ? prev : next;
    });
  }, [visible]);

  // Ctrl+N opens the Add dialog from anywhere.
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "n") {
        e.preventDefault();
        openAdd("file");
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [openAdd]);

  const resumeIds = eligible(downloads, selected, canResume);
  const pauseIds = eligible(downloads, selected, canPause);
  const cancelIds = eligible(downloads, selected, canCancel);

  async function act(action: BulkAction, ids: string[]) {
    if (ids.length === 0) return;
    setActionError(await run(action, ids));
  }

  async function removeSelected() {
    setConfirmOpen(false);
    await act("remove", [...selected]);
    setSelected(new Set());
  }

  function activate(d: Download) {
    if (canResume(d)) void act("resume", [d.id]);
    else if (canPause(d)) void act("pause", [d.id]);
  }

  const running = downloads.filter((d) => d.status === "downloading");
  const totalSpeed = running.reduce((sum, d) => sum + (progress[d.id]?.speed ?? 0), 0);

  function requestRemove() {
    if (selected.size === 0) return;
    if (settings?.confirmBeforeRemove ?? true) setConfirmOpen(true);
    else void removeSelected();
  }

  const fatal = settingsError ?? loadError;
  const title = view === "settings" ? "Settings" : view === "browser" ? "Browser integration" : LIST_VIEW_TITLES[view];

  return (
    <div className="app">
      <Sidebar view={view} counts={counts} onNavigate={setView} onOpenVideo={() => openAdd("media")} />

      <main className="main">
        <header className="topbar">
          <h1>{title}</h1>
          {view !== "settings" && view !== "browser" && (
            <div className="toolbar" role="toolbar" aria-label="Download actions">
              <label className="search">
                <Search size={15} aria-hidden />
                <input
                  type="search"
                  placeholder="Search downloads"
                  aria-label="Search downloads"
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                />
              </label>
              <button
                type="button"
                className="icon-button"
                disabled={resumeIds.length === 0}
                onClick={() => act("resume", resumeIds)}
                title="Start or resume"
                aria-label="Resume"
              >
                <Play size={17} />
              </button>
              <button
                type="button"
                className="icon-button"
                disabled={pauseIds.length === 0}
                onClick={() => act("pause", pauseIds)}
                title="Pause"
                aria-label="Pause"
              >
                <Pause size={17} />
              </button>
              <button
                type="button"
                className="icon-button"
                disabled={cancelIds.length === 0}
                onClick={() => act("cancel", cancelIds)}
                title="Cancel and discard partial data"
                aria-label="Cancel"
              >
                <Square size={15} />
              </button>
              <button
                type="button"
                className="icon-button"
                disabled={selected.size === 0}
                onClick={requestRemove}
                title="Remove from list (Delete)"
                aria-label="Remove from list"
              >
                <Trash2 size={17} />
              </button>
              <button type="button" className="button primary" onClick={() => openAdd("file")} title="Add download (Ctrl+N)">
                <Plus size={17} aria-hidden />
                Add download
              </button>
            </div>
          )}
        </header>

        {fatal && (
          <p className="banner error" role="alert">
            {fatal.message}
          </p>
        )}
        {actionError && (
          <p className="banner error" role="alert">
            {actionError}
          </p>
        )}

        {view === "settings" ? (
          settings && <SettingsView settings={settings} onUpdate={update} />
        ) : view === "browser" ? (
          settings && <BrowserView settings={settings} onUpdate={update} />
        ) : (
          <div className="content">
            {view === "all" && <DashboardStats counts={counts} />}
            {loading ? null : visible.length > 0 ? (
              <DownloadTable
                downloads={visible}
                progress={progress}
                selected={selected}
                onSelectionChange={setSelected}
                onDeleteRequest={requestRemove}
                onActivate={activate}
              />
            ) : (
              <EmptyState filtered={downloads.length > 0} onAdd={() => openAdd("file")} />
            )}
          </div>
        )}

        <footer className="statusbar">
          <span>
            {downloads.length} {downloads.length === 1 ? "download" : "downloads"}
            {selected.size > 0 && ` · ${selected.size} selected`}
          </span>
          <span>{running.length > 0 ? `${running.length} active · ${formatSpeed(totalSpeed)}` : "Idle"}</span>
        </footer>
      </main>

      {settings && (
        <AddDownloadDialog
          key={`${addTab}-${addMediaUrl ?? ""}-${addOpen}`}
          open={addOpen}
          initialTab={addTab}
          initialMediaUrl={addMediaUrl}
          defaultDir={settings.defaultDownloadDir}
          onClose={() => setAddOpen(false)}
          onAdd={add}
          onAddMedia={addMedia}
        />
      )}
      <Toasts toasts={toasts} onDismiss={dismissToast} />
      <ConfirmDialog
        open={confirmOpen}
        title={selected.size === 1 ? "Remove download?" : `Remove ${selected.size} downloads?`}
        message="They will be removed from the list. Unfinished partial files are deleted; finished files stay on disk."
        confirmLabel="Remove"
        onConfirm={removeSelected}
        onCancel={() => setConfirmOpen(false)}
      />
    </div>
  );
}

function EmptyState({ filtered, onAdd }: { filtered: boolean; onAdd: () => void }) {
  return (
    <div className="empty">
      <Logo size={64} tile />
      <h2>{filtered ? "Nothing here" : "No downloads yet"}</h2>
      <p>{filtered ? "No downloads match this view or search." : "Paste a link, send one from your browser, or grab a video."}</p>
      {!filtered && <p className="tagline">{TAGLINE}</p>}
      {!filtered && (
        <button type="button" className="button primary" onClick={onAdd}>
          <Plus size={17} aria-hidden />
          Add download
        </button>
      )}
    </div>
  );
}
