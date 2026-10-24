//! Building yt-dlp command lines from [`MediaOptions`].

use std::path::Path;

use super::output::{FILE_TEMPLATE, INFO_TEMPLATE, POSTPROCESS_TEMPLATE, PROGRESS_TEMPLATE};
use super::tools::ToolPaths;
use crate::error::{AppError, AppResult};
use crate::models::{AudioFormat, Container, MediaOptions};

/// Output name inside the temp folder; the final name may gain " (1)" etc.
const OUTPUT_TEMPLATE: &str = "%(title).150B [%(id)s].%(ext)s";

/// Rejects anything but plain format ids (and `+` / `/` combinations), so a
/// stored option can't smuggle extra command-line flags.
pub fn validate(options: &MediaOptions) -> AppResult<()> {
    match options {
        MediaOptions::Video { max_height: Some(h), .. } if *h == 0 || *h > 10_000 => {
            Err(AppError::Validation("Invalid video height.".into()))
        }
        MediaOptions::Format { format_id, .. }
            if format_id.is_empty()
                || format_id.len() > 64
                || format_id.starts_with('-')
                || !format_id.chars().all(|c| c.is_ascii_alphanumeric() || "-_.+/".contains(c)) =>
        {
            Err(AppError::Validation("Invalid format id.".into()))
        }
        _ => Ok(()),
    }
}

fn container(c: Container) -> &'static str {
    match c {
        Container::Mp4 => "mp4",
        Container::Mkv => "mkv",
    }
}

/// `-f` selector and post-processing flags.
pub fn format_args(options: &MediaOptions) -> Vec<String> {
    let s = |v: &str| v.to_string();
    match options {
        MediaOptions::Video { max_height, container: c } => {
            let cap = max_height.map(|h| format!("[height<={h}]")).unwrap_or_default();
            let res = max_height.map_or_else(|| s("res"), |h| format!("res:{h}"));
            // Resolution always ranks first, so asking for 4K never quietly
            // falls back to a lower-resolution H.264 stream.
            let sort = match c {
                // Most compatible: H.264 video with stereo AAC where offered.
                Container::Mp4 => format!("{res},vcodec:h264,channels:2,acodec:aac"),
                // Any codec: best quality at the chosen resolution.
                Container::Mkv => res,
            };
            vec![s("-f"), format!("bv*{cap}+ba/b{cap}"), s("-S"), sort, s("--merge-output-format"), s(container(*c))]
        }
        MediaOptions::Audio { format } => {
            let mut v = vec![s("-f"), s("ba/b"), s("-x")];
            let target = match format {
                AudioFormat::Best => None,
                AudioFormat::Mp3 => Some("mp3"),
                AudioFormat::M4a => Some("m4a"),
                AudioFormat::Opus => Some("opus"),
            };
            if let Some(t) = target {
                v.extend([s("--audio-format"), s(t), s("--audio-quality"), s("0")]);
            }
            v
        }
        MediaOptions::Format { format_id, container: c } => {
            vec![s("-f"), format_id.clone(), s("--merge-output-format"), s(container(*c))]
        }
    }
}

/// The complete argument list for one media download.
pub fn download_args(
    tools: &ToolPaths,
    options: &MediaOptions,
    temp_dir: &Path,
    fragments: u32,
    referrer: Option<&str>,
    url: &str,
) -> Vec<String> {
    let s = |v: &str| v.to_string();
    let mut args = vec![
        s("--ignore-config"),
        s("--no-playlist"),
        s("--newline"),
        s("--no-color"),
        s("--progress"),
        s("--progress-template"),
        s(PROGRESS_TEMPLATE),
        s("--progress-template"),
        s(POSTPROCESS_TEMPLATE),
        s("--print"),
        s(INFO_TEMPLATE),
        s("--print"),
        s(FILE_TEMPLATE),
        s("--no-simulate"),
        s("-P"),
        temp_dir.to_string_lossy().into_owned(),
        s("-o"),
        s(OUTPUT_TEMPLATE),
        s("--windows-filenames"),
        s("--ffmpeg-location"),
        tools.ffmpeg.to_string_lossy().into_owned(),
        s("-N"),
        fragments.clamp(1, 16).to_string(),
    ];
    if let Some(deno) = &tools.deno {
        args.extend([s("--js-runtimes"), format!("deno:{}", deno.display())]);
    }
    if let Some(r) = referrer.filter(|r| !r.is_empty()) {
        args.extend([s("--referer"), s(r)]);
    }
    args.extend(format_args(options));
    // "--" ends option parsing, so the URL can never be read as a flag.
    args.extend([s("--"), s(url)]);
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn video_selectors() {
        let mp4 = format_args(&MediaOptions::Video { max_height: Some(1080), container: Container::Mp4 });
        assert_eq!(
            mp4,
            ["-f", "bv*[height<=1080]+ba/b[height<=1080]", "-S", "res:1080,vcodec:h264,channels:2,acodec:aac", "--merge-output-format", "mp4"]
        );
        let best = format_args(&MediaOptions::Video { max_height: None, container: Container::Mkv });
        assert_eq!(best, ["-f", "bv*+ba/b", "-S", "res", "--merge-output-format", "mkv"]);
    }

    #[test]
    fn audio_selectors() {
        assert_eq!(
            format_args(&MediaOptions::Audio { format: AudioFormat::Mp3 }),
            ["-f", "ba/b", "-x", "--audio-format", "mp3", "--audio-quality", "0"]
        );
        assert_eq!(format_args(&MediaOptions::Audio { format: AudioFormat::Best }), ["-f", "ba/b", "-x"]);
    }

    #[test]
    fn rejects_unsafe_format_ids() {
        let f = |id: &str| validate(&MediaOptions::Format { format_id: id.into(), container: Container::Mp4 });
        assert!(f("299+ba").is_ok());
        assert!(f("18").is_ok());
        assert!(f("--exec calc").is_err());
        assert!(f("-x").is_err());
        assert!(f("a b").is_err());
        assert!(f("").is_err());
        assert!(validate(&MediaOptions::Video { max_height: Some(0), container: Container::Mp4 }).is_err());
    }

    #[test]
    fn url_comes_after_double_dash() {
        let tools = ToolPaths { yt_dlp: PathBuf::from("y.exe"), ffmpeg: PathBuf::from("C:\\t\\ffmpeg.exe"), deno: Some(PathBuf::from("C:\\t\\deno.exe")) };
        let args = download_args(&tools, &MediaOptions::Audio { format: AudioFormat::Best }, Path::new("C:\\tmp"), 4, Some("https://e.com/page"), "https://e.com/v");
        assert!(args.windows(2).any(|w| w == ["--referer", "https://e.com/page"]));
        let n = args.len();
        assert_eq!(&args[n - 2..], ["--", "https://e.com/v"]);
        assert!(args.windows(2).any(|w| w == ["--js-runtimes", "deno:C:\\t\\deno.exe"]));
        assert!(args.windows(2).any(|w| w == ["-N", "4"]));
        assert!(args.contains(&"--ignore-config".to_string()));
    }
}
