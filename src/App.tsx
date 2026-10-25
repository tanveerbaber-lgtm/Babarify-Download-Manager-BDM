import { useCallback, useEffect, useMemo, useState } from "react";
import { Eraser, ListPlus, Pause, Play, Plus, Search, Square, Trash2 } from "lucide-react";
import { Sidebar, describeSchedule } from "./components/Sidebar";
import { DashboardStats } from "./components/DashboardStats";
import { DownloadTable } from "./components/DownloadTable";
import { AddDownloadDialog } from "./components/AddDownloadDialog";
import { BatchDialog } from "./components/BatchDialog";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { ContextMenu, type MenuEntry } from "./components/ContextMenu";
import { QueueDialog } from "./components/QueueDialog";
import { SpeedLimitDialog } from "./components/SpeedLimitDialog";
import { SettingsView } from "./components/SettingsView";
import { BrowserView } from "./components/BrowserView";
import { Toasts, type ToastMessage } from "./components/Toast";
import { Logo, TAGLINE } from "./components/Logo";
import type { Tab } from "./components/AddDownloadDialog";
import { listen } from "@tauri-apps/api/event";
import { api, EVENTS } from "./lib/api";
import { useDownloads, type BulkAction } from "./hooks/useDownloads";
import { useQueues } from "./hooks/useQueues";
import { useSettings } from "./hooks/useSettings";
import { countByView, inView, isListView, LIST_VIEW_TITLES, OTHER_CATEGORY, type StatusView, type View } from "./lib/views";
import { canCancel, canPause, canRedownload, canResume, eligible } from "./lib/actions";
import { formatSpeed } from "./lib/format";
import type { AppError, BrowserMedia, Download, Priority, Queue } from "./lib/types";

export default function App() {
  const { downloads, progress, loading, error: loadError, add, addMedia, addBatch, merge, forget, run } = useDownloads();
  const { settings, error: settingsError, update } = useSettings();
  const { queues, setQueues, categories, refresh: refreshQueues } = useQueues();
  const [view, setView] = useState<View>("all");
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [addOpen, setAddOpen] = useState(false);
  const [addTab, setAddTab] = useState<Tab>("file");
  const [addMediaUrl, setAddMediaUrl] = useState<string | undefined>();
  const [batchOpen, setBatchOpen] = useState(false);
  /** `undefined` = closed, `null` = new queue. */
  const [editingQueue, setEditingQueue] = useState<Queue | null | undefined>(undefined);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const [limitOpen, setLimitOpen] = useState(false);
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
  const queueById = useMemo(() => Object.fromEntries(queues.map((q) => [q.id, q])), [queues]);
  const currentQueue = view.startsWith("queue:") ? queueById[view.slice(6)] : undefined;

  // Leave the view of a queue that was just deleted.
  useEffect(() => {
    if (view.startsWith("queue:") && queues.length > 0 && !currentQueue) setView("all");
  }, [view, queues, currentQueue]);

  const visible = useMemo(() => {
    if (!isListView(view)) return [];
    const q = query.trim().toLowerCase();
    const list = downloads.filter(
      (d) =>
        inView(d, view) &&
        (!q || d.fileName.toLowerCase().includes(q) || d.url.toLowerCase().includes(q) || (d.title ?? "").toLowerCase().includes(q)),
    );
    // A queue lists its downloads in the order they'll start.
    if (view.startsWith("queue:")) list.sort((a, b) => b.priority - a.priority || a.position - b.position);
    return list;
  }, [downloads, view, query]);

  // Drop selections that are no longer visible.
  useEffect(() => {
    setSelected((prev) => {
      const ids = new Set(visible.map((d) => d.id));
      const next = new Set([...prev].filter((id) => ids.has(id)));
      return next.size === prev.size ? prev : next;
    });
  }, [visible]);

  // Ctrl+N opens the Add dialog from anywhere; Ctrl+B the batch dialog.
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (!(e.ctrlKey || e.metaKey)) return;
      const key = e.key.toLowerCase();
      if (key === "n") {
        e.preventDefault();
        openAdd("file");
      } else if (key === "b") {
        e.preventDefault();
        setBatchOpen(true);
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [openAdd]);

  const resumeIds = eligible(downloads, selected, canResume);
  const pauseIds = eligible(downloads, selected, canPause);
  const cancelIds = eligible(downloads, selected, canCancel);
  const selectedDownloads = downloads.filter((d) => selected.has(d.id));

  async function act(action: BulkAction, ids: string[]) {
    if (ids.length === 0) return;
    setActionError(await run(action, ids));
  }

  /** Runs a bulk command that returns updated records. */
  async function bulk(call: () => Promise<Download[]>, done?: string) {
    try {
      merge(await call());
      setActionError(null);
      if (done) notify(done);
    } catch (err) {
      setActionError((err as AppError).message);
    }
  }

  async function removeSelected() {
    setConfirmOpen(false);
    await act("remove", [...selected]);
    setSelected(new Set());
  }

  async function clearFinished() {
    try {
      const gone = await api.clearFinished();
      forget(gone);
      notify(gone.length ? `Cleared ${gone.length} finished ${gone.length === 1 ? "download" : "downloads"}. The files are still on disk.` : "Nothing to clear.");
    } catch (err) {
      setActionError((err as AppError).message);
    }
  }

  async function toggleQueue(q: Queue) {
    try {
      setQueues(q.running ? await api.stopQueue(q.id) : await api.startQueue(q.id));
      setActionError(null);
    } catch (err) {
      setActionError((err as AppError).message);
    }
  }

  async function openFile(d: Download) {
    try {
      await api.openDownload(d.id);
    } catch (err) {
      setActionError((err as AppError).message);
    }
  }

  async function showInFolder(d: Download) {
    try {
      await api.showInFolder(d.id);
    } catch (err) {
      setActionError((err as AppError).message);
    }
  }

  async function copyLinks(list: Download[]) {
    try {
      await navigator.clipboard.writeText(list.map((d) => d.url).join("\n"));
      notify(list.length === 1 ? "Link copied." : `${list.length} links copied.`);
    } catch {
      setActionError("Couldn't copy to the clipboard.");
    }
  }

  async function copyChecksum(d: Download) {
    try {
      notify("Calculating SHA-256…");
      const hash = await api.fileSha256(d.id);
      await navigator.clipboard.writeText(hash);
      notify(`SHA-256 copied: ${hash.slice(0, 16)}…`);
    } catch (err) {
      setActionError((err as AppError).message);
    }
  }

  function activate(d: Download) {
    if (d.status === "completed") void openFile(d);
    else if (canResume(d)) void act("resume", [d.id]);
    else if (canPause(d)) void act("pause", [d.id]);
  }

  function menuEntries(): MenuEntry[] {
    const sel = selectedDownloads;
    if (sel.length === 0) return [];
    const ids = sel.map((d) => d.id);
    const one = sel.length === 1 ? sel[0] : null;
    const samePriority = sel.every((d) => d.priority === sel[0].priority) ? sel[0].priority : null;
    const sameQueue = sel.every((d) => d.queueId === sel[0].queueId) ? sel[0].queueId : null;
    const redownloadIds = sel.filter(canRedownload).map((d) => d.id);
    const priorities: [Priority, string][] = [[1, "High"], [0, "Normal"], [-1, "Low"]];
    return [
      { label: "Open", disabled: !one || one.status !== "completed", onSelect: () => one && openFile(one) },
      { label: "Show in Folder", disabled: !one, onSelect: () => one && showInFolder(one) },
      { label: one ? "Copy Link" : `Copy ${sel.length} Links`, onSelect: () => copyLinks(sel) },
      { label: "Copy SHA-256 Checksum", disabled: !one || one.status !== "completed", onSelect: () => one && copyChecksum(one) },
      null,
      { label: "Start / Resume", disabled: resumeIds.length === 0, onSelect: () => act("resume", resumeIds) },
      { label: "Pause", disabled: pauseIds.length === 0, onSelect: () => act("pause", pauseIds) },
      { label: "Download Again", disabled: redownloadIds.length === 0, onSelect: () => bulk(() => api.redownload(redownloadIds)) },
      null,
      {
        label: "Priority",
        items: priorities.map(([p, label]) => ({ label, checked: samePriority === p, onSelect: () => bulk(() => api.setPriority(ids, p)) })),
      },
      {
        label: "Move to Queue",
        items: queues.map((q) => ({ label: q.name, checked: sameQueue === q.id, onSelect: () => bulk(() => api.moveToQueue(ids, q.id), `Moved to ${q.name}.`) })),
      },
      { label: "Move to Front", onSelect: () => bulk(() => api.moveInQueue(ids, true)) },
      { label: "Move to Back", onSelect: () => bulk(() => api.moveInQueue(ids, false)) },
      { label: "Speed Limit…", onSelect: () => setLimitOpen(true) },
      null,
      { label: "Remove from List", danger: true, onSelect: requestRemove },
    ];
  }

  const running = downloads.filter((d) => d.status === "downloading");
  const totalSpeed = running.reduce((sum, d) => sum + (progress[d.id]?.speed ?? 0), 0);

  function requestRemove() {
    if (selected.size === 0) return;
    if (settings?.confirmBeforeRemove ?? true) setConfirmOpen(true);
    else void removeSelected();
  }

  const sharedLimit = selectedDownloads.length > 0 && selectedDownloads.every((d) => d.speedLimit === selectedDownloads[0].speedLimit) ? selectedDownloads[0].speedLimit : null;

  const fatal = settingsError ?? loadError;
  let title: string;
  if (view === "settings") title = "Settings";
  else if (view === "browser") title = "Browser Integration";
  else if (view.startsWith("queue:")) title = currentQueue?.name ?? "Queue";
  else if (view.startsWith("category:")) {
    const id = view.slice(9);
    title = id === OTHER_CATEGORY ? "Other" : (categories.find((c) => c.id === id)?.name ?? "Category");
  } else title = LIST_VIEW_TITLES[view as StatusView];

  return (
    <div className="app">
      <Sidebar
        view={view}
        counts={counts}
        queues={queues}
        categories={categories}
        onNavigate={setView}
        onOpenVideo={() => openAdd("media")}
        onToggleQueue={toggleQueue}
        onEditQueue={setEditingQueue}
      />

      <main className="main">
        <header className="topbar">
          <div className="title-group">
            <h1>{title}</h1>
            {currentQueue && (
              <div className="queue-head">
                <span className={currentQueue.running ? "queue-pill running" : "queue-pill"}>{describeSchedule(currentQueue)}</span>
                <button type="button" className="button small" onClick={() => toggleQueue(currentQueue)}>
                  {currentQueue.running ? <Pause size={14} aria-hidden /> : <Play size={14} aria-hidden />}
                  {currentQueue.running ? "Stop Queue" : "Start Queue"}
                </button>
                <button type="button" className="button small" onClick={() => setEditingQueue(currentQueue)}>
                  Schedule…
                </button>
              </div>
            )}
          </div>
          {isListView(view) && (
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
              <button
                type="button"
                className="icon-button"
                disabled={counts.status.completed === 0 && !downloads.some((d) => d.status === "cancelled")}
                onClick={clearFinished}
                title="Clear finished downloads from the list (files stay on disk)"
                aria-label="Clear finished"
              >
                <Eraser size={17} />
              </button>
              <button type="button" className="button" onClick={() => setBatchOpen(true)} title="Batch download (Ctrl+B)">
                <ListPlus size={17} aria-hidden />
                Batch
              </button>
              <button type="button" className="button primary" onClick={() => openAdd("file")} title="Add download (Ctrl+N)">
                <Plus size={17} aria-hidden />
                Add Download
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
          settings && <SettingsView settings={settings} categories={categories} onUpdate={update} onCategoriesChanged={refreshQueues} />
        ) : view === "browser" ? (
          settings && <BrowserView settings={settings} onUpdate={update} />
        ) : (
          <div className="content">
            {view === "all" && <DashboardStats counts={counts.status} />}
            {loading ? null : visible.length > 0 ? (
              <DownloadTable
                downloads={visible}
                progress={progress}
                selected={selected}
                queues={queueById}
                onSelectionChange={setSelected}
                onDeleteRequest={requestRemove}
                onActivate={activate}
                onContextMenu={(x, y) => setMenu({ x, y })}
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
          <span>
            {settings && settings.speedLimitKbps > 0 && `Limit ${formatSpeed(settings.speedLimitKbps * 1024)} · `}
            {running.length > 0 ? `${running.length} active · ${formatSpeed(totalSpeed)}` : "Idle"}
          </span>
        </footer>
      </main>

      {settings && (
        <AddDownloadDialog
          key={`${addTab}-${addMediaUrl ?? ""}-${addOpen}`}
          open={addOpen}
          initialTab={addTab}
          initialMediaUrl={addMediaUrl}
          defaultDir={settings.defaultDownloadDir}
          sortIntoCategories={settings.sortIntoCategories}
          queues={queues}
          onClose={() => setAddOpen(false)}
          onAdd={add}
          onAddMedia={addMedia}
        />
      )}
      <BatchDialog key={`batch-${batchOpen}`} open={batchOpen} queues={queues} onClose={() => setBatchOpen(false)} onAdd={addBatch} />
      {editingQueue !== undefined && (
        <QueueDialog
          queue={editingQueue}
          onClose={() => setEditingQueue(undefined)}
          onSaved={(moved) => {
            void refreshQueues();
            if (moved) merge(moved);
          }}
        />
      )}
      {menu && <ContextMenu x={menu.x} y={menu.y} entries={menuEntries()} onClose={() => setMenu(null)} />}
      <SpeedLimitDialog
        key={`limit-${limitOpen}`}
        open={limitOpen}
        count={selected.size}
        current={sharedLimit ? Math.round(sharedLimit / 1024) : null}
        onClose={() => setLimitOpen(false)}
        onApply={(kbps) => bulk(() => api.setSpeedLimit([...selected], kbps), kbps ? `Limited to ${formatSpeed(kbps * 1024)}.` : "Speed limit removed.")}
      />
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
          Add Download
        </button>
      )}
    </div>
  );
}
