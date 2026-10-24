import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, EVENTS } from "../lib/api";
import type { AppError, ToolKind, ToolProgress, ToolStatus } from "../lib/types";

/** Status of yt-dlp / FFmpeg / Deno, with install and update actions. */
export function useMediaTools() {
  const [tools, setTools] = useState<ToolStatus[] | null>(null);
  const [installing, setInstalling] = useState(false);
  const [progress, setProgress] = useState<ToolProgress | null>(null);
  const [outdated, setOutdated] = useState<ToolKind[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setTools(await api.getMediaTools());
    } catch (err) {
      setError((err as AppError).message);
    }
  }, []);

  useEffect(() => {
    void refresh();
    const unlisten = listen<ToolProgress>(EVENTS.toolProgress, ({ payload }) => setProgress(payload));
    return () => void unlisten.then((stop) => stop());
  }, [refresh]);

  const install = useCallback(async (kinds?: ToolKind[]) => {
    setInstalling(true);
    setError(null);
    try {
      setTools(await api.installMediaTools(kinds));
      setOutdated(null);
      return true;
    } catch (err) {
      setError((err as AppError).message);
      return false;
    } finally {
      setInstalling(false);
      setProgress(null);
    }
  }, []);

  const checkUpdates = useCallback(async () => {
    setError(null);
    try {
      setOutdated(await api.checkMediaToolUpdates());
    } catch (err) {
      setError((err as AppError).message);
    }
  }, []);

  const ready = tools?.every((t) => !t.required || t.source !== "missing") ?? false;
  return { tools, ready, installing, progress, outdated, error, refresh, install, checkUpdates };
}
