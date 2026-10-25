import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, EVENTS } from "../lib/api";
import type { AppError, BatchRequest, Download, NewDownloadRequest, NewMediaRequest, Progress } from "../lib/types";

export type BulkAction = "resume" | "pause" | "cancel" | "remove";

/** Engine events are applied in batches this often. */
const FLUSH_MS = 50;

function upsert(list: Download[], d: Download): Download[] {
  const i = list.findIndex((x) => x.id === d.id);
  if (i === -1) return [d, ...list];
  const next = list.slice();
  next[i] = d;
  return next;
}

function omit<T>(map: Record<string, T>, key: string): Record<string, T> {
  if (!(key in map)) return map;
  const next = { ...map };
  delete next[key];
  return next;
}

export function useDownloads() {
  const [downloads, setDownloads] = useState<Download[]>([]);
  const [progress, setProgress] = useState<Record<string, Progress>>({});
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<AppError | null>(null);

  const refresh = useCallback(async () => {
    try {
      setDownloads(await api.listDownloads());
      setError(null);
    } catch (err) {
      setError(err as AppError);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // Live updates from the engine, applied together every FLUSH_MS so a bulk
  // action on hundreds of downloads re-renders the list once, not per event.
  useEffect(() => {
    const fresh = () => ({ updated: new Map<string, Download>(), removed: new Set<string>(), progress: new Map<string, Progress>() });
    let pending = fresh();
    let timer: ReturnType<typeof setTimeout> | undefined;

    const flush = () => {
      timer = undefined;
      const { updated, removed, progress: live } = pending;
      pending = fresh();
      if (updated.size > 0 || removed.size > 0) {
        setDownloads((prev) => {
          let next = removed.size > 0 ? prev.filter((d) => !removed.has(d.id)) : prev;
          for (const d of updated.values()) if (!removed.has(d.id)) next = upsert(next, d);
          return next;
        });
      }
      setProgress((prev) => {
        let next = prev;
        if (live.size > 0) {
          next = { ...prev };
          for (const p of live.values()) next[p.id] = p;
        }
        for (const d of updated.values()) if (d.status !== "downloading") next = omit(next, d.id);
        for (const id of removed) next = omit(next, id);
        return next;
      });
    };
    const schedule = () => {
      timer ??= setTimeout(flush, FLUSH_MS);
    };

    const unlisten = [
      listen<Download>(EVENTS.updated, ({ payload }) => {
        pending.updated.set(payload.id, payload);
        schedule();
      }),
      listen<string>(EVENTS.removed, ({ payload }) => {
        pending.removed.add(payload);
        schedule();
      }),
      listen<Progress>(EVENTS.progress, ({ payload }) => {
        pending.progress.set(payload.id, payload);
        schedule();
      }),
    ];
    return () => {
      if (timer) clearTimeout(timer);
      for (const p of unlisten) void p.then((stop) => stop());
    };
  }, []);

  const add = useCallback(async (request: NewDownloadRequest) => {
    const created = await api.addDownload(request);
    // The engine may already have reported a newer state for this download
    // (e.g. "downloading") before the command returned; keep that one.
    setDownloads((prev) => (prev.some((d) => d.id === created.id) ? prev : [created, ...prev]));
    return created;
  }, []);

  const addMedia = useCallback(async (request: NewMediaRequest) => {
    const created = await api.addMediaDownloads(request);
    setDownloads((prev) => {
      const known = new Set(prev.map((d) => d.id));
      return [...created.filter((d) => !known.has(d.id)), ...prev];
    });
    return created;
  }, []);

  const addBatch = useCallback(async (request: BatchRequest) => {
    const created = await api.addBatch(request);
    setDownloads((prev) => {
      const known = new Set(prev.map((d) => d.id));
      return [...created.filter((d) => !known.has(d.id)).reverse(), ...prev];
    });
    return created;
  }, []);

  /** Applies records returned by a bulk command. */
  const merge = useCallback((updated: Download[]) => {
    setDownloads((prev) => updated.reduce((acc, d) => upsert(acc, d), prev));
  }, []);

  /** Drops records the backend removed (e.g. "clear finished"). */
  const forget = useCallback((ids: string[]) => {
    const gone = new Set(ids);
    setDownloads((prev) => prev.filter((d) => !gone.has(d.id)));
  }, []);

  /** Runs an action on every id in one call; returns the first error message, if any. */
  const run = useCallback(async (action: BulkAction, ids: string[]) => {
    try {
      const firstError = await api.bulkAction(action, ids);
      if (action === "remove") {
        const gone = new Set(ids);
        setDownloads((prev) => prev.filter((d) => !gone.has(d.id)));
      }
      return firstError;
    } catch (err) {
      return (err as AppError).message;
    }
  }, []);

  return { downloads, progress, loading, error, refresh, add, addMedia, addBatch, merge, forget, run };
}
