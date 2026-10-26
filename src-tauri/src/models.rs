//! Data types exchanged between the Rust core and the frontend.
//!
//! Every type here has a TypeScript mirror in `src/lib/types.ts`; keep the
//! two in sync when changing fields.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DownloadStatus {
    Pending,
    Queued,
    Downloading,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

impl DownloadStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Queued => "queued",
            Self::Downloading => "downloading",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "pending" => Self::Pending,
            "queued" => Self::Queued,
            "downloading" => Self::Downloading,
            "paused" => Self::Paused,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DownloadKind {
    /// An ordinary file fetched by the HTTP engine.
    File,
    /// Video or audio fetched through yt-dlp.
    Media,
}

impl DownloadKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Media => "media",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Container {
    Mp4,
    Mkv,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AudioFormat {
    /// Keep the site's audio as is (no re-encoding).
    Best,
    Mp3,
    M4a,
    Opus,
}

/// What to fetch for a media download.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "camelCase", deny_unknown_fields)]
pub enum MediaOptions {
    /// Best video up to `max_height` (any height when `None`) plus best audio.
    #[serde(rename_all = "camelCase")]
    Video { max_height: Option<u32>, container: Container },
    /// Audio only.
    #[serde(rename_all = "camelCase")]
    Audio { format: AudioFormat },
    /// One exact format from the site's list (video-only formats get the best audio added).
    #[serde(rename_all = "camelCase")]
    Format { format_id: String, container: Container },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    pub id: String,
    pub kind: DownloadKind,
    pub url: String,
    pub file_name: String,
    /// Title reported by the site (media downloads).
    pub title: Option<String>,
    pub media_options: Option<MediaOptions>,
    pub destination_dir: String,
    pub status: DownloadStatus,
    pub total_bytes: Option<i64>,
    pub downloaded_bytes: i64,
    pub error_message: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
    /// URL after redirects, once the server has been contacted.
    pub final_url: Option<String>,
    /// Full path of the finished file.
    pub saved_path: Option<String>,
    /// Whether the server supports byte ranges (pause/resume without restarting).
    pub resumable: bool,
    pub expected_sha256: Option<String>,
    /// Page that linked to the file (sent as Referer).
    pub referrer: Option<String>,
    pub queue_id: String,
    /// -1 low, 0 normal, 1 high.
    pub priority: i32,
    /// Order within its queue (lower starts first at equal priority).
    pub position: i64,
    /// Category id (e.g. "videos"); `None` for other files.
    pub category: Option<String>,
    /// Per-download speed limit in bytes/s.
    pub speed_limit: Option<i64>,
    /// Automatic retries already used after failures.
    pub auto_retries: i64,
    /// When the next automatic retry is due (unix ms).
    pub retry_at: Option<i64>,
    /// Virus scan of the finished file: "scanning", "clean", "threat" or "unavailable".
    pub scan_result: Option<String>,
    #[serde(skip)]
    pub temp_path: Option<String>,
    #[serde(skip)]
    pub etag: Option<String>,
    #[serde(skip)]
    pub last_modified: Option<String>,
}

/// A byte range of a segmented download. `end` is inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    pub idx: u32,
    pub start: u64,
    pub end: u64,
    pub downloaded: u64,
}

impl Segment {
    pub fn len(&self) -> u64 {
        self.end - self.start + 1
    }

    pub fn remaining(&self) -> u64 {
        self.len().saturating_sub(self.downloaded)
    }
}

/// Payload of the `add_download` command.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewDownloadRequest {
    pub url: String,
    /// Optional override; derived from the URL when omitted or blank.
    pub file_name: Option<String>,
    /// Optional override; falls back to the default download folder setting.
    pub destination_dir: Option<String>,
    /// Optional SHA-256 (hex) to verify the finished file against.
    pub expected_sha256: Option<String>,
    /// Start immediately (default) or leave the download pending.
    pub start: Option<bool>,
    /// The page that linked to the file (sent as Referer, origin-only across sites).
    pub referrer: Option<String>,
    /// Category id chosen in the dialog; detected from the name when omitted.
    pub category: Option<String>,
    /// Queue to add it to; Main when omitted.
    pub queue_id: Option<String>,
    /// -1 low, 0 normal (default), 1 high.
    pub priority: Option<i32>,
    /// The download's own speed limit in KB/s.
    pub speed_limit_kbps: Option<u32>,
}

/// When a queue may start downloads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueueSchedule {
    /// Off = the queue runs whenever it's started.
    pub enabled: bool,
    /// Local time "HH:MM".
    pub start: String,
    /// Local time "HH:MM"; `None` = keep going until the queue is empty.
    pub stop: Option<String>,
    /// Days the window opens on: 0 = Monday … 6 = Sunday.
    pub days: Vec<u8>,
}

impl Default for QueueSchedule {
    fn default() -> Self {
        Self { enabled: false, start: "00:00".into(), stop: None, days: (0..7).collect() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Queue {
    pub id: String,
    pub name: String,
    pub schedule: QueueSchedule,
    pub running: bool,
    pub sort_order: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub id: String,
    pub name: String,
    /// Lowercase extensions without dots.
    pub extensions: Vec<String>,
    /// Absolute folder; `None` = a subfolder of the default download folder.
    pub folder: Option<String>,
    pub sort_order: i64,
}

/// What BDM does when every download has finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WhenDone {
    Nothing,
    Sleep,
    Shutdown,
    Exit,
}

/// What happens when the browser sends a link BDM has already downloaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DuplicateAction {
    /// Download it again (saved as "name (1).ext").
    Add,
    /// Skip it and say so.
    Skip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub theme: Theme,
    pub default_download_dir: String,
    pub max_concurrent_downloads: u32,
    pub segments_per_download: u32,
    pub confirm_before_remove: bool,
    /// Keep BDM's own copy of yt-dlp up to date (checked at most daily).
    pub auto_update_yt_dlp: bool,
    /// Bring BDM to the front when the browser sends it something.
    pub show_on_browser_add: bool,
    /// Global speed limit in KB/s; 0 = unlimited.
    pub speed_limit_kbps: u32,
    /// Retries within one attempt (dropped connections, timeouts, 5xx).
    pub max_retries: u32,
    /// Automatic retries of a failed download.
    pub auto_retry_count: u32,
    /// Minutes before an automatic retry.
    pub auto_retry_delay_minutes: u32,
    /// Put downloads in their category's folder.
    pub sort_into_categories: bool,
    pub notify_on_complete: bool,
    pub notify_on_failure: bool,
    pub when_all_done: WhenDone,
    pub duplicate_action: DuplicateAction,
    /// Allow links to this PC and the local network (off by default).
    pub allow_local_network: bool,
    /// Ask before starting downloads sent by the browser (the "New Download" dialog).
    pub confirm_browser_downloads: bool,
    /// Open a download's own window when it starts from that dialog.
    pub show_download_window: bool,
    /// Closing the main window keeps BDM running in the tray.
    pub close_to_tray: bool,
    /// Start BDM (in the tray) when signing in to Windows.
    pub start_with_windows: bool,
    /// Offer to download links to files when they're copied.
    pub offer_copied_links: bool,
    /// Scan finished downloads with Microsoft Defender.
    pub scan_downloads: bool,
}

/// Partial update for settings; only provided fields are changed.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsPatch {
    pub theme: Option<Theme>,
    pub default_download_dir: Option<String>,
    pub max_concurrent_downloads: Option<u32>,
    pub segments_per_download: Option<u32>,
    pub confirm_before_remove: Option<bool>,
    pub auto_update_yt_dlp: Option<bool>,
    pub show_on_browser_add: Option<bool>,
    pub speed_limit_kbps: Option<u32>,
    pub max_retries: Option<u32>,
    pub auto_retry_count: Option<u32>,
    pub auto_retry_delay_minutes: Option<u32>,
    pub sort_into_categories: Option<bool>,
    pub notify_on_complete: Option<bool>,
    pub notify_on_failure: Option<bool>,
    pub when_all_done: Option<WhenDone>,
    pub duplicate_action: Option<DuplicateAction>,
    pub allow_local_network: Option<bool>,
    pub confirm_browser_downloads: Option<bool>,
    pub show_download_window: Option<bool>,
    pub close_to_tray: Option<bool>,
    pub start_with_windows: Option<bool>,
    pub offer_copied_links: Option<bool>,
    pub scan_downloads: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub data_dir: String,
    pub schema_version: u32,
}
