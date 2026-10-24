// Typed wrappers around the Rust commands in `src-tauri/src/commands.rs`.
// Components call these instead of `invoke` directly.

import { invoke } from "@tauri-apps/api/core";
import type {
  AppError,
  AppInfo,
  BatchRequest,
  BrowserStatus,
  Category,
  Download,
  MediaInfo,
  NewDownloadRequest,
  NewMediaRequest,
  Priority,
  Queue,
  QueueSchedule,
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

  addBatch: (request: BatchRequest) => call<Download[]>("add_batch", { request }),
  expandBatchPattern: (pattern: string) => call<string[]>("expand_batch_pattern", { pattern }),
  importUrlList: (path: string) => call<string[]>("import_url_list", { path }),
  findDuplicate: (url: string) => call<Download | null>("find_duplicate", { url }),

  listQueues: () => call<Queue[]>("list_queues"),
  saveQueue: (name: string, schedule: QueueSchedule, id?: string) => call<Queue>("save_queue", { id, name, schedule }),
  deleteQueue: (id: string) => call<Download[]>("delete_queue", { id }),
  startQueue: (id: string) => call<Queue[]>("start_queue", { id }),
  stopQueue: (id: string) => call<Queue[]>("stop_queue", { id }),

  listCategories: () => call<Category[]>("list_categories"),
  saveCategory: (name: string, extensions: string[], folder: string | null, id?: string) =>
    call<Category>("save_category", { id, name, extensions, folder }),
  deleteCategory: (id: string) => call<void>("delete_category", { id }),

  setPriority: (ids: string[], priority: Priority) => call<Download[]>("set_download_priority", { ids, priority }),
  moveToQueue: (ids: string[], queueId: string) => call<Download[]>("move_downloads_to_queue", { ids, queueId }),
  moveInQueue: (ids: string[], front: boolean) => call<Download[]>("move_downloads_in_queue", { ids, front }),
  setSpeedLimit: (ids: string[], kbps: number | null) => call<Download[]>("set_download_speed_limit", { ids, kbps }),
  redownload: (ids: string[]) => call<Download[]>("redownload", { ids }),
  clearFinished: () => call<string[]>("clear_finished"),
  openDownload: (id: string) => call<void>("open_download", { id }),
  showInFolder: (id: string) => call<void>("show_in_folder", { id }),
};

/** Event names emitted by the Rust engine (`src-tauri/src/engine/events.rs`). */
export const EVENTS = {
  progress: "download:progress",
  updated: "download:updated",
  removed: "download:removed",
  toolProgress: "tools:progress",
  browserAdded: "browser:added",
  browserMedia: "browser:media",
  queuesChanged: "queues:changed",
} as const;
