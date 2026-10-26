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

/// `--referer` and `--add-header` flags. Headers the browser sent for this
/// video win; otherwise the referrer follows the browser's default policy.
pub fn request_args(referrer: Option<&str>, url: &str, headers: &[(String, String)]) -> Vec<String> {
    let mut args = Vec::new();
    if !headers.iter().any(|(n, _)| n.eq_ignore_ascii_case("referer"))
        && let Some(r) = crate::engine::http::referrer_for(referrer.filter(|r| !r.is_empty()), url)
    {
        args.extend(["--referer".to_string(), r]);
    }
    for (name, value) in headers {
        args.extend(["--add-header".to_string(), format!("{name}:{value}")]);
    }
    args
}

/// The complete argument list for one media download.
pub fn download_args(
    tools: &ToolPaths,
    options: &MediaOptions,
    temp_dir: &Path,
    fragments: u32,
    referrer: Option<&str>,
    headers: &[(String, String)],
    url: &str,
) -> Vec<String> {
    let s = |v: &str| v.to_string();
    let mut args = vec![
        s("--ignore-config"),
        // Plugins from the user's yt-dlp folders could run arbitrary code.
        s("--no-plugin-dirs"),
        // Titles and paths in UTF-8, not the Windows code page: "’" or Urdu
        // text would otherwise arrive mangled or as "?".
        s("--encoding"),
        s("utf-8"),
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
    args.extend(request_args(referrer, url, headers));
    args.extend(format_args(options));
    // "--" ends option parsing, so the URL can never be read as a flag.
    args.extend([s("--"), s(url)]);
    args
}

/// What's added to video downloads besides the video itself (Settings →
/// Video Downloads).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaExtras {
    pub chapters: bool,
    /// The video's thumbnail as cover art.
    pub thumbnail: bool,
    pub subtitles: Option<SubtitleChoice>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtitleChoice {
    /// yt-dlp language patterns, e.g. ["en.*", "ur.*"].
    pub languages: Vec<String>,
    /// Also the site's automatic captions (YouTube's auto-generated ones).
    pub automatic: bool,
}

impl MediaExtras {
    pub fn from_settings(s: &crate::models::Settings) -> Self {
        Self {
            chapters: s.embed_chapters,
            thumbnail: s.embed_thumbnail,
            subtitles: s.download_subtitles.then(|| SubtitleChoice {
                languages: parse_languages(&s.subtitle_languages).unwrap_or_else(|_| vec!["en".into()]),
                automatic: s.automatic_subtitles,
            }),
        }
    }

    pub fn without_subtitles(&self) -> Self {
        Self { subtitles: None, ..self.clone() }
    }
}

/// "en, ur" → ["en", "ur"]: exact codes. yt-dlp matches patterns ignoring
/// case, so anything wider (like "en.*") also picks up the machine
/// translations sites list as en-bn, en-de… (dozens of tracks, which get
/// rate-limited). Regional variants are named outright (en-GB, pt-BR);
/// "all" and patterns with "*" are kept as written.
pub fn parse_languages(text: &str) -> Result<Vec<String>, String> {
    let codes: Vec<&str> = text.split([',', ' ', ';']).map(str::trim).filter(|c| !c.is_empty()).collect();
    if codes.is_empty() {
        return Err("Enter at least one subtitle language, e.g. en.".into());
    }
    if codes.len() > 10 {
        return Err("Choose at most 10 subtitle languages.".into());
    }
    let patterns: Vec<String> = codes
        .iter()
        .map(|c| {
            let ok = c.len() <= 20
                && c.chars().next().is_some_and(|ch| ch.is_ascii_alphanumeric())
                && c.chars().all(|ch| ch.is_ascii_alphanumeric() || "-_.*".contains(ch));
            if !ok {
                return Err(format!("\"{c}\" isn't a language code (use codes like en, ur, hi or pt-BR)."));
            }
            Ok((*c).to_string())
        })
        .collect::<Result<_, String>>()?;
    Ok(patterns)
}

/// yt-dlp flags for the extras. Subtitles only go with video.
pub fn extras_args(options: &MediaOptions, extras: &MediaExtras) -> Vec<String> {
    let s = |v: &str| v.to_string();
    let mut args = Vec::new();
    if extras.chapters {
        args.push(s("--embed-chapters"));
    }
    if extras.thumbnail {
        args.push(s("--embed-thumbnail"));
    }
    if let Some(subs) = &extras.subtitles
        && !matches!(options, MediaOptions::Audio { .. })
    {
        args.push(s("--write-subs"));
        if subs.automatic {
            args.push(s("--write-auto-subs"));
        }
        args.extend([s("--sub-langs"), subs.languages.join(","), s("--embed-subs")]);
    }
    args
}

/// Speed limit (bytes/s) and retry counts for yt-dlp.
pub fn limit_args(rate: Option<u64>, retries: u32) -> Vec<String> {
    let mut args = vec!["-R".to_string(), retries.to_string(), "--fragment-retries".into(), retries.to_string()];
    if let Some(r) = rate.filter(|r| *r > 0) {
        args.extend(["-r".to_string(), r.to_string()]);
    }
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
    fn captured_headers_replace_the_guessed_referrer() {
        let headers = vec![
            ("Referer".to_string(), "https://site.com/watch?v=1".to_string()),
            ("Origin".to_string(), "https://site.com".to_string()),
        ];
        assert_eq!(
            request_args(Some("https://site.com/other"), "https://cdn.net/v.m3u8", &headers),
            ["--add-header", "Referer:https://site.com/watch?v=1", "--add-header", "Origin:https://site.com"]
        );
        assert_eq!(request_args(Some("https://site.com/p"), "https://cdn.net/v", &[]), ["--referer", "https://site.com/"]);
    }

    #[test]
    fn extras_become_flags() {
        let video = MediaOptions::Video { max_height: Some(720), container: Container::Mp4 };
        let all = MediaExtras {
            chapters: true,
            thumbnail: true,
            subtitles: Some(SubtitleChoice { languages: vec!["en".into(), "ur".into()], automatic: true }),
        };
        assert_eq!(
            extras_args(&video, &all),
            ["--embed-chapters", "--embed-thumbnail", "--write-subs", "--write-auto-subs", "--sub-langs", "en,ur", "--embed-subs"]
        );
        let audio = MediaOptions::Audio { format: AudioFormat::Mp3 };
        assert_eq!(extras_args(&audio, &all), ["--embed-chapters", "--embed-thumbnail"], "no subtitles in audio files");
        assert!(extras_args(&video, &MediaExtras::default()).is_empty());
        assert_eq!(all.without_subtitles().subtitles, None);
    }

    #[test]
    fn subtitle_languages_are_checked() {
        assert_eq!(parse_languages("en, ur").unwrap(), ["en", "ur"]);
        assert_eq!(parse_languages("pt-BR all en.*").unwrap(), ["pt-BR", "all", "en.*"]);
        assert!(parse_languages("  ").is_err());
        assert!(parse_languages("en --exec calc").is_err(), "no flags");
        assert!(parse_languages("e n,a,b,c,d,f,g,h,i,j,k").is_err(), "too many");
    }

    #[test]
    fn url_comes_after_double_dash() {
        let tools = ToolPaths { yt_dlp: PathBuf::from("y.exe"), ffmpeg: PathBuf::from("C:\\t\\ffmpeg.exe"), deno: Some(PathBuf::from("C:\\t\\deno.exe")) };
        let args = download_args(&tools, &MediaOptions::Audio { format: AudioFormat::Best }, Path::new("C:\\tmp"), 4, Some("https://e.com/page"), &[], "https://e.com/v");
        assert!(args.windows(2).any(|w| w == ["--referer", "https://e.com/page"]));
        let other = download_args(&tools, &MediaOptions::Audio { format: AudioFormat::Best }, Path::new("C:\\tmp"), 4, Some("https://site.com/watch?session=secret"), &[], "https://cdn.net/v");
        assert!(other.windows(2).any(|w| w == ["--referer", "https://site.com/"]), "only the origin goes to another site");
        let n = args.len();
        assert_eq!(&args[n - 2..], ["--", "https://e.com/v"]);
        assert_eq!(limit_args(Some(524_288), 3), ["-R", "3", "--fragment-retries", "3", "-r", "524288"]);
        assert_eq!(limit_args(None, 0).len(), 4);
        assert!(args.windows(2).any(|w| w == ["--js-runtimes", "deno:C:\\t\\deno.exe"]));
        assert!(args.windows(2).any(|w| w == ["-N", "4"]));
        assert!(args.contains(&"--ignore-config".to_string()));
        assert!(args.windows(2).any(|w| w == ["--encoding", "utf-8"]));
    }
}
