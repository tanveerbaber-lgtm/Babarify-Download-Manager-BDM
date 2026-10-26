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

/** "scanning" while Microsoft Defender checks a finished file. */
export type ScanResult = "scanning" | "clean" | "threat" | "unavailable";

export interface Download {
  /** Virus scan of the finished file. */
  scanResult: ScanResult | null;
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
  /** Page that linked to the file. */
  referrer: string | null;
  queueId: string;
  /** -1 low, 0 normal, 1 high. */
  priority: Priority;
  /** Order within the queue; lower starts first at equal priority. */
  position: number;
  /** Category id, e.g. "videos"; null for other files. */
  category: string | null;
  /** The download's own speed limit in bytes/s. */
  speedLimit: number | null;
  /** Automatic retries already used. */
  autoRetries: number;
  /** When the next automatic retry is due (unix ms). */
  retryAt: number | null;
}

export type Priority = -1 | 0 | 1;

export interface QueueSchedule {
  enabled: boolean;
  /** Local time "HH:MM". */
  start: string;
  /** Local time "HH:MM"; null keeps going until the queue is empty. */
  stop: string | null;
  /** 0 = Monday … 6 = Sunday. */
  days: number[];
}

export interface Queue {
  id: string;
  name: string;
  schedule: QueueSchedule;
  running: boolean;
  sortOrder: number;
}

export interface Category {
  id: string;
  name: string;
  extensions: string[];
  /** Absolute folder; null = a subfolder of the default download folder. */
  folder: string | null;
  sortOrder: number;
}

export interface BatchRequest {
  urls: string[];
  destinationDir?: string;
  queueId?: string;
  start?: boolean;
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
  /** Per-connection progress of a segmented download. */
  segments: SegmentView[];
}

export interface SegmentView {
  start: number;
  len: number;
  done: number;
}

export interface NewDownloadRequest {
  url: string;
  fileName?: string;
  destinationDir?: string;
  /** Optional SHA-256 (hex) to verify the finished file. */
  expectedSha256?: string;
  /** Start right away (default) or leave pending. */
  start?: boolean;
  queueId?: string;
  priority?: Priority;
  /** The download's own speed limit in KB/s. */
  speedLimitKbps?: number;
  /** Page that linked to the file. */
  referrer?: string;
  /** Category id chosen by the user ("other" = none). */
  category?: string;
}

export type WhenDone = "nothing" | "sleep" | "shutdown" | "exit";
export type DuplicateAction = "add" | "skip";

export type Theme = "system" | "light" | "dark";

export interface Settings {
  theme: Theme;
  defaultDownloadDir: string;
  maxConcurrentDownloads: number;
  segmentsPerDownload: number;
  confirmBeforeRemove: boolean;
  autoUpdateYtDlp: boolean;
  showOnBrowserAdd: boolean;
  /** KB/s; 0 = unlimited. */
  speedLimitKbps: number;
  maxRetries: number;
  autoRetryCount: number;
  autoRetryDelayMinutes: number;
  sortIntoCategories: boolean;
  notifyOnComplete: boolean;
  notifyOnFailure: boolean;
  whenAllDone: WhenDone;
  duplicateAction: DuplicateAction;
  /** Allow links to this PC and the local network. */
  allowLocalNetwork: boolean;
  /** Ask before starting downloads sent by the browser. */
  confirmBrowserDownloads: boolean;
  /** Open a download's own window when it starts from the dialog. */
  showDownloadWindow: boolean;
  /** Closing the main window keeps BDM running in the tray. */
  closeToTray: boolean;
  /** Start BDM in the tray when signing in to Windows. */
  startWithWindows: boolean;
  /** Offer to download links to files when they're copied. */
  offerCopiedLinks: boolean;
  /** Scan finished downloads with Microsoft Defender. */
  scanDownloads: boolean;
  /** Video downloads: chapter markers in the file. */
  embedChapters: boolean;
  /** Video and audio downloads: the thumbnail as cover art. */
  embedThumbnail: boolean;
  /** Video downloads: subtitles in `subtitleLanguages`, embedded. */
  downloadSubtitles: boolean;
  /** e.g. "en, ur". */
  subtitleLanguages: string;
  /** Use automatic captions when a language has no real subtitles. */
  automaticSubtitles: boolean;
}

/** What BDM was asked to show by the command line or a bdm:// link before the window loaded. */
export type UiRequest =
  | { kind: "confirm"; proposal: BrowserProposal }
  | { kind: "media"; url: string }
  | { kind: "import"; links: string[] }
  | { kind: "added"; download: Download };

/** The copied link shown in the "Download with BDM?" window. */
export interface ClipboardOffer {
  url: string;
  fileName: string;
  host: string;
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
  items: { url: string; title?: string; referrer?: string }[];
  options: MediaOptions;
  destinationDir?: string;
  start?: boolean;
  category?: string;
}

/** Payload of `browser:confirm`: a browser request waiting for the user. */
export interface BrowserProposal {
  kind: "file" | "media";
  url: string;
  fileName: string;
  size: number | null;
  referrer: string | null;
  /** Detected category id, or "other". */
  category: string;
  title: string | null;
  options: MediaOptions | null;
  /** e.g. "MP4 · 1080p". */
  label: string | null;
  duplicate: Download | null;
  /** An unfinished download with the same name and size, which this link could continue. */
  resumeCandidate: Download | null;
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
