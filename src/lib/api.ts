// Typed wrappers around the Rust commands in `src-tauri/src/commands.rs`.
// Components call these instead of `invoke` directly.

import { invoke } from "@tauri-apps/api/core";
import type {
  AppError,
  AppInfo,
  BrowserStatus,
  Download,
  MediaInfo,
  NewDownloadRequest,
  NewMediaRequest,
  ToolKind,
  ToolStatus,
  Settings,
  SettingsPatch,
} from "./types";

function isAppError(value: unknown): value is AppError {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as AppError).kind === "string" &&
    typeof (value as AppError).message === "string"
  );
}

/** Normalizes anything thrown by `invoke` into an `AppError`. */
export function toAppError(err: unknown): AppError {
  if (isAppError(err)) return err;
  return { kind: "internal", message: err instanceof Error ? err.message : String(err) };
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (err) {
    throw toAppError(err);
  }
}

export const api = {
  getAppInfo: () => call<AppInfo>("get_app_info"),
  listDownloads: () => call<Download[]>("list_downloads"),
  addDownload: (request: NewDownloadRequest) => call<Download>("add_download", { request }),
  resumeDownload: (id: string) => call<void>("resume_download", { id }),
  pauseDownload: (id: string) => call<void>("pause_download", { id }),
  cancelDownload: (id: string) => call<void>("cancel_download", { id }),
  removeDownload: (id: string) => call<void>("remove_download", { id }),
  suggestFileName: (url: string) => call<string>("suggest_file_name", { url }),
  getSettings: () => call<Settings>("get_settings"),
  updateSettings: (patch: SettingsPatch) => call<Settings>("update_settings", { patch }),
  getMediaTools: () => call<ToolStatus[]>("get_media_tools"),
  installMediaTools: (kinds?: ToolKind[]) => call<ToolStatus[]>("install_media_tools", { kinds }),
  checkMediaToolUpdates: () => call<ToolKind[]>("check_media_tool_updates"),
  analyzeMedia: (url: string) => call<MediaInfo>("analyze_media", { url }),
  addMediaDownloads: (request: NewMediaRequest) => call<Download[]>("add_media_downloads", { request }),
  getBrowserIntegration: () => call<BrowserStatus[]>("get_browser_integration"),
  registerBrowserIntegration: () => call<BrowserStatus[]>("register_browser_integration"),
  unregisterBrowserIntegration: () => call<BrowserStatus[]>("unregister_browser_integration"),
};

/** Event names emitted by the Rust engine (`src-tauri/src/engine/events.rs`). */
export const EVENTS = {
  progress: "download:progress",
  updated: "download:updated",
  removed: "download:removed",
  toolProgress: "tools:progress",
  browserAdded: "browser:added",
  browserMedia: "browser:media",
} as const;
