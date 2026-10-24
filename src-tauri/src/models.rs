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
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub data_dir: String,
    pub schema_version: u32,
}
