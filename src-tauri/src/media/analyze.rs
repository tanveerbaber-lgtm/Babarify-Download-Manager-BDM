//! Turning `yt-dlp -J` output into choices the UI can offer.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use super::output::friendly_error;
use super::process;
use super::tools::ToolPaths;
use crate::error::{AppError, AppResult};

const ANALYZE_TIMEOUT: Duration = Duration::from_secs(120);
pub const MAX_PLAYLIST_ENTRIES: usize = 1000;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MediaInfo {
    /// "video" or "playlist".
    pub kind: &'static str,
    pub url: String,
    pub title: String,
    pub uploader: Option<String>,
    pub duration: Option<f64>,
    pub site: String,
    pub license: Option<String>,
    pub is_live: bool,
    /// Every format is DRM-protected, so nothing can be downloaded.
    pub drm_protected: bool,
    /// Video heights on offer, highest first.
    pub qualities: Vec<QualityOption>,
    /// Best audio-only stream, if the site offers one.
    pub audio: Option<AudioOption>,
    /// The full format list for advanced selection.
    pub formats: Vec<FormatInfo>,
    pub entries: Vec<PlaylistEntry>,
    /// Entries beyond [`MAX_PLAYLIST_ENTRIES`] are not listed.
    pub entries_truncated: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QualityOption {
    pub height: u32,
    pub label: String,
    pub fps: Option<f64>,
    /// Expected size with the MP4 (most compatible) choice, if sizes are known.
    pub estimated_bytes: Option<u64>,
    /// Expected size with the MKV (best quality, any codec) choice.
    pub estimated_bytes_best: Option<u64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioOption {
    pub bitrate_kbps: Option<f64>,
    pub estimated_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FormatInfo {
    pub id: String,
    pub ext: String,
    /// "video", "audio" or "both".
    pub content: &'static str,
    pub height: Option<u32>,
    pub fps: Option<f64>,
    pub vcodec: Option<String>,
    pub acodec: Option<String>,
    pub bitrate_kbps: Option<f64>,
    pub size_bytes: Option<u64>,
    /// Size was worked out from bitrate × duration rather than reported.
    pub size_is_estimate: bool,
    pub audio_channels: Option<u32>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistEntry {
    pub url: String,
    pub title: String,
    pub duration: Option<f64>,
}

/// Runs yt-dlp in metadata-only mode.
pub async fn analyze(tools: &ToolPaths, url: &str) -> AppResult<MediaInfo> {
    let mut cmd = process::command(&tools.yt_dlp);
    cmd.args(["--ignore-config", "--no-warnings", "-J", "--flat-playlist", "--no-playlist"]);
    add_js_runtime(&mut cmd, tools.deno.as_deref());
    cmd.args(["--", url]);

    let out = tokio::time::timeout(ANALYZE_TIMEOUT, cmd.output())
        .await
        .map_err(|_| AppError::Validation("The site took too long to respond. Try again.".into()))?
        .map_err(|e| AppError::Io(format!("couldn't run yt-dlp: {e}")))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let lines: Vec<String> = stderr.lines().map(str::to_owned).collect();
        return Err(AppError::Validation(friendly_error(&lines)));
    }
    let json: Value = serde_json::from_slice(&out.stdout)
        .map_err(|_| AppError::Validation("yt-dlp returned data BDM couldn't read.".into()))?;
    parse(&json, url)
}

pub fn add_js_runtime(cmd: &mut tokio::process::Command, deno: Option<&Path>) {
    if let Some(deno) = deno {
        cmd.arg("--js-runtimes").arg(format!("deno:{}", deno.display()));
    }
}

pub fn parse(json: &Value, original_url: &str) -> AppResult<MediaInfo> {
    let is_playlist = json["_type"].as_str() == Some("playlist");
    let title = str_of(json, "title").unwrap_or_else(|| "Untitled".into());
    let mut info = MediaInfo {
        kind: if is_playlist { "playlist" } else { "video" },
        url: str_of(json, "webpage_url").unwrap_or_else(|| original_url.to_string()),
        title,
        uploader: str_of(json, "uploader").or_else(|| str_of(json, "channel")),
        duration: json["duration"].as_f64(),
        site: str_of(json, "extractor_key").or_else(|| str_of(json, "extractor")).unwrap_or_default(),
        license: str_of(json, "license"),
        is_live: json["is_live"].as_bool().unwrap_or(false) || json["live_status"].as_str() == Some("is_live"),
        drm_protected: false,
        qualities: Vec::new(),
        audio: None,
        formats: Vec::new(),
        entries: Vec::new(),
        entries_truncated: false,
    };

    if is_playlist {
        let entries = json["entries"].as_array().map(Vec::as_slice).unwrap_or_default();
        info.entries_truncated = entries.len() > MAX_PLAYLIST_ENTRIES;
        info.entries = entries
            .iter()
            .take(MAX_PLAYLIST_ENTRIES)
            .filter_map(|e| {
                let url = str_of(e, "url").or_else(|| str_of(e, "webpage_url"))?;
                url.starts_with("http").then(|| PlaylistEntry {
                    url,
                    title: str_of(e, "title").unwrap_or_else(|| "Untitled".into()),
                    duration: e["duration"].as_f64(),
                })
            })
            .collect();
        if info.entries.is_empty() {
            return Err(AppError::Validation("This playlist has no downloadable videos.".into()));
        }
        return Ok(info);
    }

    let all = json["formats"].as_array().map(Vec::as_slice).unwrap_or_default();
    let media: Vec<&Value> = all.iter().filter(|f| is_media_format(f)).collect();
    let usable: Vec<&Value> = media.iter().copied().filter(|f| !f["has_drm"].as_bool().unwrap_or(false)).collect();
    info.drm_protected = !media.is_empty() && usable.is_empty();

    // Some sites return a single URL with no format list.
    if all.is_empty() && json["url"].is_string() {
        info.formats.push(format_info(json, info.duration));
    } else {
        info.formats = usable.iter().map(|f| format_info(f, info.duration)).collect();
    }
    info.formats.sort_by(|a, b| {
        (b.height.unwrap_or(0), b.bitrate_kbps.unwrap_or(0.0) as u64)
            .cmp(&(a.height.unwrap_or(0), a.bitrate_kbps.unwrap_or(0.0) as u64))
    });

    let best_audio = info
        .formats
        .iter()
        .filter(|f| f.content == "audio")
        .max_by(|a, b| a.bitrate_kbps.unwrap_or(0.0).total_cmp(&b.bitrate_kbps.unwrap_or(0.0)));
    info.audio = best_audio.map(|a| AudioOption { bitrate_kbps: a.bitrate_kbps, estimated_bytes: a.size_bytes });

    // Estimates mirror the format sorting passed to yt-dlp (see args.rs):
    // MP4 ranks H.264 and stereo AAC first; MKV uses yt-dlp's defaults
    // (frame rate, then codec; channels, then codec). Formats with a
    // reported size are preferred over guessed ones.
    let audio = || info.formats.iter().filter(|f| f.content == "audio");
    let compat_audio = pick_by(audio(), |f| (sized(f), f.audio_channels.is_none_or(|c| c <= 2), is_aac(f), br(f)));
    let best_audio_any = pick_by(audio(), |f| (sized(f), f.audio_channels.unwrap_or(2), audio_rank(f), br(f)));
    let estimate = |video: Option<&FormatInfo>, audio: Option<&FormatInfo>| {
        let audio_size = if video.is_some_and(|v| v.content == "both") { Some(0) } else { audio.and_then(|a| a.size_bytes) };
        video.and_then(|v| v.size_bytes).zip(audio_size).map(|(v, a)| v + a)
    };

    let mut by_height: BTreeMap<u32, Vec<&FormatInfo>> = BTreeMap::new();
    for f in info.formats.iter().filter(|f| f.content != "audio") {
        if let Some(h) = f.height {
            by_height.entry(h).or_default().push(f);
        }
    }
    info.qualities = by_height
        .into_iter()
        .rev()
        .map(|(height, fs)| {
            let compat = pick_by(fs.iter().copied(), |f| (sized(f), is_h264(f), fps_of(f), br(f)));
            let best = pick_by(fs.iter().copied(), |f| (sized(f), fps_of(f), video_rank(f), br(f)));
            let fps = fs.iter().filter_map(|f| f.fps).fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.max(x))));
            QualityOption {
                height,
                label: quality_label(height),
                fps,
                estimated_bytes: estimate(compat, compat_audio),
                estimated_bytes_best: estimate(best, best_audio_any),
            }
        })
        .collect();
    Ok(info)
}

/// The format with the highest sort key.
fn pick_by<'a, K: PartialOrd>(formats: impl Iterator<Item = &'a FormatInfo>, key: impl Fn(&FormatInfo) -> K) -> Option<&'a FormatInfo> {
    formats.max_by(|a, b| key(a).partial_cmp(&key(b)).unwrap_or(std::cmp::Ordering::Equal))
}

fn sized(f: &FormatInfo) -> bool {
    f.size_bytes.is_some() && !f.size_is_estimate
}

fn br(f: &FormatInfo) -> f64 {
    f.bitrate_kbps.unwrap_or(0.0)
}

fn fps_of(f: &FormatInfo) -> u32 {
    f.fps.unwrap_or(0.0).round() as u32
}

fn is_h264(f: &FormatInfo) -> bool {
    f.vcodec.as_deref().is_some_and(|c| c.starts_with("avc1") || c.starts_with("h264"))
}

fn is_aac(f: &FormatInfo) -> bool {
    f.acodec.as_deref().is_some_and(|c| c.starts_with("mp4a") || c == "aac")
}

/// yt-dlp's default video codec preference.
fn video_rank(f: &FormatInfo) -> u8 {
    match f.vcodec.as_deref().unwrap_or_default() {
        c if c.starts_with("av01") => 4,
        c if c.starts_with("vp9") || c.starts_with("vp09") => 3,
        c if c.starts_with("hev") || c.starts_with("hvc") || c.starts_with("h265") => 2,
        c if c.starts_with("avc") || c.starts_with("h264") => 1,
        _ => 0,
    }
}

/// yt-dlp's default audio codec preference (lossy codecs).
fn audio_rank(f: &FormatInfo) -> u8 {
    match f.acodec.as_deref().unwrap_or_default() {
        "opus" => 4,
        "vorbis" => 3,
        c if c.starts_with("mp4a") || c == "aac" => 2,
        "mp3" => 1,
        _ => 0,
    }
}

fn is_media_format(f: &Value) -> bool {
    let protocol = f["protocol"].as_str().unwrap_or_default();
    let note = f["format_note"].as_str().unwrap_or_default();
    let none = |k: &str| f[k].as_str() == Some("none");
    protocol != "mhtml" && !note.contains("storyboard") && !(none("vcodec") && none("acodec"))
}

fn format_info(f: &Value, duration: Option<f64>) -> FormatInfo {
    let codec = |k: &str| str_of(f, k).filter(|c| c != "none");
    let vcodec = codec("vcodec");
    let acodec = codec("acodec");
    let height = f["height"].as_u64().map(|h| h as u32);
    // yt-dlp omits codec fields it doesn't know; infer what we can.
    let has_video = match f["vcodec"].as_str() {
        Some(c) => c != "none",
        None => height.is_some(),
    };
    let has_audio = match f["acodec"].as_str() {
        Some(c) => c != "none",
        None => f["audio_channels"].is_number() || f["vcodec"].as_str() == Some("none"),
    };
    let content = match (has_video, has_audio) {
        (true, false) => "video",
        (false, true) => "audio",
        // Both, or nothing known (e.g. a plain media URL): treat as complete.
        _ => "both",
    };
    let bitrate = f["tbr"].as_f64().or_else(|| f["abr"].as_f64().filter(|b| *b > 0.0));
    let reported = f["filesize"].as_u64().or_else(|| f["filesize_approx"].as_u64());
    let size = reported.or_else(|| bitrate.zip(duration).map(|(kbps, secs)| (kbps * 1000.0 / 8.0 * secs) as u64));
    FormatInfo {
        id: str_of(f, "format_id").unwrap_or_else(|| "0".into()),
        ext: str_of(f, "ext").unwrap_or_default(),
        content,
        height,
        fps: f["fps"].as_f64(),
        vcodec,
        acodec,
        bitrate_kbps: bitrate,
        size_bytes: size,
        size_is_estimate: reported.is_none(),
        audio_channels: f["audio_channels"].as_u64().map(|c| c as u32),
        note: str_of(f, "format_note"),
    }
}

pub fn quality_label(height: u32) -> String {
    match height {
        4320 => "4320p (8K)".into(),
        2160 => "2160p (4K)".into(),
        1440 => "1440p (2K)".into(),
        h => format!("{h}p"),
    }
}

fn str_of(v: &Value, key: &str) -> Option<String> {
    v[key].as_str().filter(|s| !s.is_empty()).map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("testsupport/fixtures").join(name);
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn lists_qualities_from_360p_to_4k() {
        let info = parse(&fixture("video.json"), "https://example.com/v").unwrap();
        assert_eq!(info.kind, "video");
        assert_eq!(info.title, "Big Buck Bunny 60fps 4K - Official Blender Foundation Short Film");
        assert_eq!(info.license.as_deref(), Some("Creative Commons Attribution license (reuse allowed)"));
        let heights: Vec<u32> = info.qualities.iter().map(|q| q.height).collect();
        assert_eq!(heights, [2160, 1440, 1080, 720, 360]);
        assert_eq!(info.qualities[0].label, "2160p (4K)");
        assert_eq!(info.qualities[0].fps, Some(60.0));
        // H.264 1080p (299) plus stereo AAC (140), as the MP4 default picks.
        assert_eq!(info.qualities[2].estimated_bytes, Some(257_619_653 + 10_271_496));
        // 360p: the combined H.264+AAC format 18 needs no extra audio.
        assert_eq!(info.qualities[4].estimated_bytes, Some(28_526_904));
        // MKV at 2160p: AV1 (401) beats VP9 (315); 6-channel AAC (258) beats stereo.
        assert_eq!(info.qualities[0].estimated_bytes_best, Some(712_445_280 + 30_767_611));
        assert!(!info.drm_protected);
    }

    #[test]
    fn excludes_storyboards_and_drm_formats() {
        let info = parse(&fixture("video.json"), "u").unwrap();
        assert!(info.formats.iter().all(|f| !f.id.starts_with("sb")));
        assert!(info.formats.iter().all(|f| f.id != "drm1"));
        let audio = info.audio.unwrap();
        assert_eq!(audio.estimated_bytes, Some(30_767_611));
        let muxed = info.formats.iter().find(|f| f.id == "18").unwrap();
        assert_eq!(muxed.content, "both");
        assert_eq!(info.formats.iter().find(|f| f.id == "299").unwrap().content, "video");
        assert_eq!(info.formats.iter().find(|f| f.id == "140").unwrap().content, "audio");
    }

    #[test]
    fn detects_drm_only_media() {
        let json = serde_json::json!({
            "_type": "video", "title": "Protected", "formats": [
                {"format_id": "a", "vcodec": "avc1", "acodec": "none", "height": 720, "has_drm": true},
                {"format_id": "b", "vcodec": "none", "acodec": "mp4a", "has_drm": true}
            ]
        });
        let info = parse(&json, "u").unwrap();
        assert!(info.drm_protected);
        assert!(info.qualities.is_empty());
    }

    #[test]
    fn parses_flat_playlists() {
        let info = parse(&fixture("playlist.json"), "u").unwrap();
        assert_eq!(info.kind, "playlist");
        assert_eq!(info.title, "Blender Open Movies");
        assert_eq!(info.entries.len(), 3);
        assert_eq!(info.entries[1].title, "Sintel");
        assert!(!info.entries_truncated);
    }

    #[test]
    fn single_url_results_get_one_format() {
        let json = serde_json::json!({"_type": "video", "title": "clip", "url": "https://cdn/x.mp4", "ext": "mp4", "format_id": "0"});
        let info = parse(&json, "u").unwrap();
        assert_eq!(info.formats.len(), 1);
    }
}
