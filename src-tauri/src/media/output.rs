//! Parsing yt-dlp's machine-readable output and turning its errors into
//! messages people can act on.

/// Templates passed to yt-dlp; fields are `|`-separated and `NA` when unknown.
pub const PROGRESS_TEMPLATE: &str = "download:BDMP|%(progress.status)s|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress.speed)s|%(progress.eta)s|%(info.format_id)s";
pub const POSTPROCESS_TEMPLATE: &str = "postprocess:BDMPP|%(progress.status)s|%(progress.postprocessor)s";
/// The size field is yt-dlp's exact or approximate size of everything it will
/// download (both streams when video and audio are merged). Title goes last: it may contain `|`.
pub const INFO_TEMPLATE: &str = "before_dl:BDMINFO|%(format_id)s|%(filesize,filesize_approx)s|%(title)s";
pub const FILE_TEMPLATE: &str = "after_move:BDMFILE|%(filepath)s";

#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    /// Byte progress of the stream currently downloading.
    Progress {
        finished: bool,
        downloaded: Option<u64>,
        total: Option<u64>,
        speed: Option<f64>,
        format_id: String,
    },
    /// A post-processing step started or finished.
    PostProcess { started: bool, name: String },
    /// Before downloading: the chosen format(s), e.g. "299+140", and the
    /// expected total size if yt-dlp knows or can estimate it.
    Info { format_id: String, expected_size: Option<u64>, title: String },
    /// The final file path after all processing.
    File(String),
    Other,
}

fn num<T: std::str::FromStr>(s: &str) -> Option<T> {
    if s == "NA" { None } else { s.parse().ok() }
}

pub fn parse_line(line: &str) -> Line {
    let line = line.trim_end_matches(['\r', '\n']);
    if let Some(rest) = line.strip_prefix("BDMP|") {
        let f: Vec<&str> = rest.splitn(7, '|').collect();
        if f.len() == 7 {
            let total = num::<f64>(f[2]).or_else(|| num::<f64>(f[3])).map(|t| t as u64);
            return Line::Progress {
                finished: f[0] == "finished",
                downloaded: num::<f64>(f[1]).map(|d| d as u64),
                total,
                speed: num(f[4]),
                format_id: f[6].to_string(),
            };
        }
    } else if let Some(rest) = line.strip_prefix("BDMPP|") {
        if let Some((status, name)) = rest.split_once('|') {
            return Line::PostProcess { started: status == "started", name: name.to_string() };
        }
    } else if let Some(rest) = line.strip_prefix("BDMINFO|") {
        let f: Vec<&str> = rest.splitn(3, '|').collect();
        if f.len() == 3 {
            return Line::Info {
                format_id: f[0].to_string(),
                expected_size: num::<f64>(f[1]).map(|s| s as u64).filter(|s| *s > 0),
                title: f[2].to_string(),
            };
        }
    } else if let Some(path) = line.strip_prefix("BDMFILE|") {
        return Line::File(path.to_string());
    }
    Line::Other
}

/// Human description of a post-processing step.
pub fn stage_label(postprocessor: &str) -> String {
    match postprocessor {
        "Merger" => "Merging video and audio".into(),
        "ExtractAudio" => "Converting audio".into(),
        "VideoRemuxer" | "VideoConvertor" => "Converting video".into(),
        "MoveFiles" => "Finishing".into(),
        p if p.starts_with("Fixup") => "Fixing up the file".into(),
        _ => "Processing".into(),
    }
}

/// The site wants a signed-in account and BDM had no sign-in for it.
pub const SIGN_IN_NEEDED: &str =
    "This site wants you signed in. Allow BDM to use your browser sign-in for this site (from the Download With BDM panel or the BDM extension's options), then try again.";
/// The site still wanted a sign-in although the browser's cookies were used.
pub const SIGN_IN_REFUSED: &str =
    "The site still wants a sign-in. Make sure you're signed in to it in this browser, then try again.";

/// yt-dlp has no extractor for the page and found no video in it.
pub const UNSUPPORTED_SITE: &str =
    "This website isn't supported for video downloads. Try adding the link as a regular file download.";

/// yt-dlp or FFmpeg ran out of disk space.
pub const DISK_FULL: &str = "The drive is full. Free up space, then resume this download.";

/// Turns yt-dlp's stderr into a short message. URLs are removed because
/// they can carry signed tokens.
pub fn friendly_error(stderr: &[String]) -> String {
    let error = stderr
        .iter()
        .rev()
        .find_map(|l| l.trim().strip_prefix("ERROR:").map(str::trim))
        .unwrap_or_else(|| stderr.iter().rev().map(|l| l.trim()).find(|l| !l.is_empty()).unwrap_or(""));
    let lower = error.to_ascii_lowercase();

    let known = if lower.contains("no space left") || lower.contains("not enough space on the disk") || lower.contains("disk full") {
        Some(DISK_FULL)
    } else if lower.contains("drm") {
        Some("This media is protected by DRM. BDM doesn't download DRM-protected content.")
    } else if lower.contains("sign in") || lower.contains("login") || lower.contains("log in") || lower.contains("members-only") || lower.contains("cookies") {
        Some(SIGN_IN_NEEDED)
    } else if lower.contains("private video") {
        Some("This video is private.")
    } else if lower.contains("unsupported url") {
        Some(UNSUPPORTED_SITE)
    } else if lower.contains("requested format is not available") {
        Some("The chosen quality isn't available for this video. Pick another option.")
    } else if lower.contains("video unavailable") || lower.contains("not available") || lower.contains("has been removed") {
        Some("This media is unavailable. It may have been removed or blocked in your region.")
    } else if lower.contains("is live") || lower.contains("live event will begin") {
        Some("Live streams aren't supported yet.")
    } else if lower.contains("http error 403") {
        Some("The site refused access (HTTP 403). Updating yt-dlp in Settings → Media tools often fixes this.")
    } else if lower.contains("http error 429") || lower.contains("too many requests") {
        Some("The site is limiting requests (HTTP 429). Try again later.")
    } else if lower.contains("unable to download") && (lower.contains("timed out") || lower.contains("connection")) {
        Some("Network problem while contacting the site. Check your connection and try again.")
    } else {
        None
    };
    if let Some(msg) = known {
        return msg.to_string();
    }

    let cleaned = strip_urls(strip_extractor_prefix(error));
    if cleaned.is_empty() {
        "yt-dlp stopped with an unknown error. If this site changed recently, updating yt-dlp may help.".into()
    } else {
        format!("{cleaned} If this site changed recently, updating yt-dlp in Settings → Media tools may help.")
    }
}

/// "[youtube] abc123: message" → "message"
fn strip_extractor_prefix(msg: &str) -> &str {
    if msg.starts_with('[')
        && let Some(end) = msg.find("] ")
    {
        let rest = &msg[end + 2..];
        return match rest.split_once(": ") {
            Some((id, tail)) if !id.contains(' ') => tail,
            _ => rest,
        };
    }
    msg
}

fn strip_urls(msg: &str) -> String {
    let mut out = msg
        .split_whitespace()
        .filter(|w| !w.contains("://"))
        .collect::<Vec<_>>()
        .join(" ");
    if !out.is_empty() && !out.ends_with(['.', '!', '?']) {
        out.push('.');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_progress_lines() {
        assert_eq!(
            parse_line("BDMP|downloading|3072|66820|NA|2555514.05|0|mp4"),
            Line::Progress { finished: false, downloaded: Some(3072), total: Some(66820), speed: Some(2555514.05), format_id: "mp4".into() }
        );
        // HLS streams only have an estimate.
        assert_eq!(
            parse_line("BDMP|downloading|1000|NA|50000.5|NA|NA|233"),
            Line::Progress { finished: false, downloaded: Some(1000), total: Some(50000), speed: None, format_id: "233".into() }
        );
        assert!(matches!(parse_line("BDMP|finished|5|5|NA|1|NA|140"), Line::Progress { finished: true, .. }));
    }

    #[test]
    fn parses_other_lines() {
        assert_eq!(parse_line("BDMPP|started|Merger"), Line::PostProcess { started: true, name: "Merger".into() });
        assert_eq!(
            parse_line("BDMINFO|299+140|1234567.8|Big | Buck"),
            Line::Info { format_id: "299+140".into(), expected_size: Some(1234567), title: "Big | Buck".into() }
        );
        assert_eq!(
            parse_line("BDMINFO|18|NA|Clip"),
            Line::Info { format_id: "18".into(), expected_size: None, title: "Clip".into() }
        );
        assert_eq!(parse_line("BDMFILE|C:\\dl\\a [b].mp4\r"), Line::File("C:\\dl\\a [b].mp4".into()));
        assert_eq!(parse_line("[download] something"), Line::Other);
    }

    #[test]
    fn maps_common_errors() {
        let e = |s: &str| friendly_error(&[s.to_string()]);
        assert!(e("ERROR: [youtube] x: This video is DRM protected").contains("DRM"));
        assert!(e("ERROR: [youtube] x: Sign in to confirm your age").contains("signed in"));
        assert!(e("ERROR: Unsupported URL: https://example.com/page").contains("isn't supported"));
        assert!(e("ERROR: [youtube] abc: Video unavailable").contains("unavailable"));
        assert!(e("ERROR: [youtube] abc: Requested format is not available").contains("quality"));
        assert!(e("ERROR: unable to download video data: HTTP Error 403: Forbidden").contains("Updating yt-dlp"));
    }

    #[test]
    fn unknown_errors_drop_prefix_and_urls() {
        let msg = friendly_error(&[
            "WARNING: something".into(),
            "ERROR: [generic] page: Failed to parse https://cdn.example.com/x?token=SECRET".into(),
        ]);
        assert!(msg.starts_with("Failed to parse."), "{msg}");
        assert!(!msg.contains("SECRET"));
    }

    #[test]
    fn stage_labels() {
        assert_eq!(stage_label("Merger"), "Merging video and audio");
        assert_eq!(stage_label("FixupM3u8"), "Fixing up the file");
    }
}
