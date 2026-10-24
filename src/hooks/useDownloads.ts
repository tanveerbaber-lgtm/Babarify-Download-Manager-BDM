import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, EVENTS } from "../lib/api";
import type { AppError, Download, NewDownloadRequest, NewMediaRequest, Progress } from "../lib/types";

export type BulkAction = "resume" | "pause" | "cancel" | "remove";

const ACTIONS: Record<BulkAction, (id: string) => Promise<void>> = {
  resume: api.resumeDownload,
  pause: api.pauseDownload,
  cancel: api.cancelDownload,
  remove: api.removeDownload,
};

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

  // Live updates from the engine.
  useEffect(() => {
    const unlisten = [
      listen<Download>(EVENTS.updated, ({ payload }) => {
        setDownloads((prev) => upsert(prev, payload));
        if (payload.status !== "downloading") {
          setProgress((prev) => omit(prev, payload.id));
        }
      }),
      listen<string>(EVENTS.removed, ({ payload }) => {
        setDownloads((prev) => prev.filter((d) => d.id !== payload));
        setProgress((prev) => omit(prev, payload));
      }),
      listen<Progress>(EVENTS.progress, ({ payload }) => {
        setProgress((prev) => ({ ...prev, [payload.id]: payload }));
      }),
    ];
    return () => {
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

  /** Runs an action on each id; returns the first error message, if any. */
  const run = useCallback(async (action: BulkAction, ids: string[]) => {
    let firstError: string | null = null;
    for (const id of ids) {
      try {
        await ACTIONS[action](id);
        if (action === "remove") setDownloads((prev) => prev.filter((d) => d.id !== id));
      } catch (err) {
        firstError ??= (err as AppError).message;
      }
    }
    return firstError;
  }, []);

  return { downloads, progress, loading, error, refresh, add, addMedia, run };
}
