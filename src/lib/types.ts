// TypeScript mirrors of the Rust types in `src-tauri/src/models.rs` and
// `src-tauri/src/error.rs`. Keep both sides in sync.

export type DownloadStatus =
  | "pending"
  | "queued"
  | "downloading"
  | "paused"
  | "completed"
  | "failed"
  | "cancelled";

export type DownloadKind = "file" | "media";

export type Container = "mp4" | "mkv";
export type AudioFormat = "best" | "mp3" | "m4a" | "opus";

/** What to fetch for a media download (mirrors `MediaOptions` in Rust). */
export type MediaOptions =
  | { mode: "video"; maxHeight: number | null; container: Container }
  | { mode: "audio"; format: AudioFormat }
  | { mode: "format"; formatId: string; container: Container };

export interface Download {
  id: string;
  kind: DownloadKind;
  url: string;
  fileName: string;
  /** Title reported by the site (media downloads). */
  title: string | null;
  mediaOptions: MediaOptions | null;
  destinationDir: string;
  status: DownloadStatus;
  totalBytes: number | null;
  downloadedBytes: number;
  errorMessage: string | null;
  /** Unix epoch milliseconds. */
  createdAt: number;
  updatedAt: number;
  completedAt: number | null;
  /** URL after redirects, once the server has been contacted. */
  finalUrl: string | null;
  /** Full path of the finished file. */
  savedPath: string | null;
  /** Server supports byte ranges, so pausing doesn't lose progress. */
  resumable: boolean;
  expectedSha256: string | null;
}

/** Payload of the `download:progress` event. */
export interface Progress {
  id: string;
  downloadedBytes: number;
  totalBytes: number | null;
  /** Bytes per second. */
  speed: number;
  etaSeconds: number | null;
  connections: number;
  /** Media stage, e.g. "Merging video and audio". */
  stage: string | null;
}

export interface NewDownloadRequest {
  url: string;
  fileName?: string;
  destinationDir?: string;
  /** Optional SHA-256 (hex) to verify the finished file. */
  expectedSha256?: string;
  /** Start right away (default) or leave pending. */
  start?: boolean;
}

export type Theme = "system" | "light" | "dark";

export interface Settings {
  theme: Theme;
  defaultDownloadDir: string;
  maxConcurrentDownloads: number;
  segmentsPerDownload: number;
  confirmBeforeRemove: boolean;
  autoUpdateYtDlp: boolean;
  showOnBrowserAdd: boolean;
}

export type SettingsPatch = Partial<Settings>;

export interface AppInfo {
  name: string;
  version: string;
  dataDir: string;
  schemaVersion: number;
}

export type AppErrorKind = "validation" | "notFound" | "database" | "io" | "internal";

export interface AppError {
  kind: AppErrorKind;
  message: string;
}

// ------------------------------------------------------------------ media

export type ToolKind = "ytDlp" | "ffmpeg" | "deno";
export type ToolSource = "managed" | "system" | "missing";

export interface ToolStatus {
  kind: ToolKind;
  name: string;
  purpose: string;
  required: boolean;
  source: ToolSource;
  path: string | null;
  version: string | null;
}

/** Payload of the `tools:progress` event. */
export interface ToolProgress {
  tool: ToolKind;
  downloadedBytes: number;
  totalBytes: number;
  stage: string;
}

export interface QualityOption {
  height: number;
  label: string;
  fps: number | null;
  /** Expected size with MP4 (most compatible). */
  estimatedBytes: number | null;
  /** Expected size with MKV (best quality). */
  estimatedBytesBest: number | null;
}

export interface FormatInfo {
  id: string;
  ext: string;
  content: "video" | "audio" | "both";
  height: number | null;
  fps: number | null;
  vcodec: string | null;
  acodec: string | null;
  bitrateKbps: number | null;
  sizeBytes: number | null;
  /** Size worked out from bitrate × duration rather than reported by the site. */
  sizeIsEstimate: boolean;
  audioChannels: number | null;
  note: string | null;
}

export interface PlaylistEntry {
  url: string;
  title: string;
  duration: number | null;
}

export interface MediaInfo {
  kind: "video" | "playlist";
  url: string;
  title: string;
  uploader: string | null;
  duration: number | null;
  site: string;
  license: string | null;
  isLive: boolean;
  drmProtected: boolean;
  qualities: QualityOption[];
  audio: { bitrateKbps: number | null; estimatedBytes: number | null } | null;
  formats: FormatInfo[];
  entries: PlaylistEntry[];
  entriesTruncated: boolean;
}

export interface NewMediaRequest {
  items: { url: string; title?: string }[];
  options: MediaOptions;
  destinationDir?: string;
  start?: boolean;
}

// ---------------------------------------------------------------- browser

export interface BrowserStatus {
  browser: "chrome" | "edge";
  name: string;
  registered: boolean;
  hostFound: boolean;
}

/** Payload of the `browser:media` event. */
export interface BrowserMedia {
  url: string;
  pageTitle: string | null;
}
