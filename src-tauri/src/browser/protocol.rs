//! Messages between the browser extension, the native host and the app.
//!
//! Browser ↔ host uses Chrome's native-messaging framing: a 4-byte
//! little-endian length followed by UTF-8 JSON. Host ↔ app uses the same
//! framing over a named pipe, with the request wrapped in a [`PipeRequest`]
//! that carries the per-user token.

use std::io::{self, Read, Write};

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::cookies::{self, BrowserCookie};
use crate::error::{AppError, AppResult};
use crate::media::args;
use crate::models::MediaOptions;
use crate::validation;

/// Largest message accepted from the browser or the pipe. Real requests are
/// a few hundred bytes; the cap stops a misbehaving peer from exhausting memory.
pub const MAX_MESSAGE: usize = 64 * 1024;

/// What the extension can ask for. Unknown types or fields are rejected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum BrowserRequest {
    /// Is BDM installed and running?
    Ping,
    /// Download a file the browser was about to download (or a link).
    #[serde(rename_all = "camelCase")]
    Download {
        url: String,
        file_name: Option<String>,
        /// The page that linked to the file; some servers require it.
        referrer: Option<String>,
        mime: Option<String>,
        size: Option<i64>,
        /// The site's sign-in cookies, only for sites the user allowed.
        #[serde(default)]
        cookies: Option<Vec<BrowserCookie>>,
    },
    /// Open BDM's video/audio dialog for a page.
    #[serde(rename_all = "camelCase")]
    Media {
        url: String,
        page_title: Option<String>,
        /// Headers the browser sent for `url` (media detection).
        #[serde(default)]
        headers: Option<BTreeMap<String, String>>,
        #[serde(default)]
        cookies: Option<Vec<BrowserCookie>>,
    },
    /// List download choices for the video on a page (on-video panel).
    #[serde(rename_all = "camelCase")]
    Formats {
        url: String,
        /// The page playing `url` when it's a stream or file taken from the player.
        #[serde(default)]
        referrer: Option<String>,
        #[serde(default)]
        headers: Option<BTreeMap<String, String>>,
        #[serde(default)]
        cookies: Option<Vec<BrowserCookie>>,
    },
    /// Start a video/audio download straight away with a choice from `Formats`.
    #[serde(rename_all = "camelCase")]
    MediaDownload {
        url: String,
        title: Option<String>,
        options: MediaOptions,
        /// The choice as shown, e.g. "MP4 · 1080p" (for the confirm dialog).
        label: Option<String>,
        size: Option<u64>,
        #[serde(default)]
        referrer: Option<String>,
        #[serde(default)]
        headers: Option<BTreeMap<String, String>>,
        #[serde(default)]
        cookies: Option<Vec<BrowserCookie>>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserResponse {
    pub ok: bool,
    /// "running" or "stopped" for pings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Download choices, in reply to `Formats`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media: Option<MediaChoices>,
    /// The request is waiting in BDM's "New Download" dialog.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub asked: bool,
    /// The site wants a sign-in BDM wasn't given; the panel offers to allow it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sign_in: bool,
    /// yt-dlp can't read the page; the panel tries the video its player is playing.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unsupported: bool,
}

/// What the on-video panel offers for one video.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaChoices {
    pub title: String,
    pub site: String,
    pub duration: Option<f64>,
    pub license: Option<String>,
    pub choices: Vec<MediaChoice>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaChoice {
    /// e.g. "MP4 · 1080p 60fps".
    pub label: String,
    /// e.g. "≈ 268 MB".
    pub size_bytes: Option<u64>,
    /// "video" or "audio".
    pub kind: String,
    /// Sent back unchanged in `MediaDownload`.
    pub options: MediaOptions,
}

impl BrowserResponse {
    pub fn ok() -> Self {
        Self { ok: true, app: None, version: None, error: None, media: None, asked: false, sign_in: false, unsupported: false }
    }

    /// Accepted, but BDM is asking the user to confirm first.
    pub fn asked() -> Self {
        Self { asked: true, ..Self::ok() }
    }

    pub fn with_media(media: MediaChoices) -> Self {
        Self { media: Some(media), ..Self::ok() }
    }

    pub fn running(version: &str) -> Self {
        Self { app: Some("running".into()), version: Some(version.into()), ..Self::ok() }
    }

    pub fn stopped() -> Self {
        Self { app: Some("stopped".into()), ..Self::ok() }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self { error: Some(message.into()), ok: false, ..Self::ok() }
    }

    /// An error, telling the panel when allowing the browser sign-in would help.
    pub fn media_error(message: String, had_cookies: bool) -> Self {
        if message == crate::media::output::UNSUPPORTED_SITE {
            return Self { unsupported: true, ..Self::error(message) };
        }
        if message == crate::media::output::SIGN_IN_NEEDED {
            if had_cookies {
                return Self::error(crate::media::output::SIGN_IN_REFUSED);
            }
            return Self { sign_in: true, ..Self::error(message) };
        }
        Self::error(message)
    }
}

/// A request forwarded over the pipe.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipeRequest {
    pub token: String,
    pub request: BrowserRequest,
}

impl BrowserRequest {
    /// Takes the sign-in cookies out of the request, with the URL they came for.
    pub fn take_cookies(&mut self) -> Option<(String, Vec<BrowserCookie>)> {
        let (url, cookies) = match self {
            Self::Ping => return None,
            Self::Download { url, cookies, .. }
            | Self::Media { url, cookies, .. }
            | Self::Formats { url, cookies, .. }
            | Self::MediaDownload { url, cookies, .. } => (url, cookies),
        };
        cookies.take().map(|c| (url.clone(), c))
    }

    /// Takes the captured request headers out of the request (validated and
    /// filtered), with the URL they were sent to.
    pub fn take_headers(&mut self) -> Option<(String, Vec<(String, String)>)> {
        let (url, headers) = match self {
            Self::Media { url, headers, .. } | Self::Formats { url, headers, .. } | Self::MediaDownload { url, headers, .. } => {
                (url, headers)
            }
            _ => return None,
        };
        let kept = crate::request_headers::sanitize(&headers.take()?).ok()?;
        Some((url.clone(), kept))
    }

    /// Checks every field before anything acts on it. The app validates
    /// again when it creates the download.
    pub fn validate(&self) -> AppResult<()> {
        if let Self::Download { url, cookies: Some(c), .. }
        | Self::Media { url, cookies: Some(c), .. }
        | Self::Formats { url, cookies: Some(c), .. }
        | Self::MediaDownload { url, cookies: Some(c), .. } = self
        {
            validation::validate_url(url)?;
            cookies::validate(c, url)?;
        }
        if let Self::Media { headers: Some(h), .. } | Self::Formats { headers: Some(h), .. } | Self::MediaDownload { headers: Some(h), .. } =
            self
        {
            crate::request_headers::sanitize(h)?;
        }
        if let Self::Formats { referrer: Some(r), .. } | Self::MediaDownload { referrer: Some(r), .. } = self
            && !r.is_empty()
        {
            validation::validate_url(r)?;
        }
        match self {
            Self::Ping => Ok(()),
            Self::Download { url, file_name, referrer, mime, .. } => {
                validation::validate_url(url)?;
                if let Some(name) = file_name.as_deref().filter(|n| !n.trim().is_empty()) {
                    // Browsers report a full path for some downloads; keep the name only.
                    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
                    if base.chars().count() > 255 {
                        return Err(AppError::Validation("File name is too long.".into()));
                    }
                }
                if let Some(r) = referrer.as_deref().filter(|r| !r.is_empty()) {
                    validation::validate_url(r)?;
                }
                if mime.as_deref().is_some_and(|m| m.len() > 255) {
                    return Err(AppError::Validation("Invalid MIME type.".into()));
                }
                Ok(())
            }
            Self::Media { url, page_title, .. } => {
                validation::validate_url(url)?;
                if page_title.as_deref().is_some_and(|t| t.chars().count() > 1000) {
                    return Err(AppError::Validation("Page title is too long.".into()));
                }
                Ok(())
            }
            Self::Formats { url, .. } => validation::validate_url(url).map(|_| ()),
            Self::MediaDownload { url, title, options, label, .. } => {
                validation::validate_url(url)?;
                if title.as_deref().is_some_and(|t| t.chars().count() > 1000) {
                    return Err(AppError::Validation("Title is too long.".into()));
                }
                if label.as_deref().is_some_and(|l| l.chars().count() > 100) {
                    return Err(AppError::Validation("Label is too long.".into()));
                }
                args::validate(options)
            }
        }
    }
}

/// Reads one framed message. `Ok(None)` means the peer closed the stream.
pub fn read_frame(reader: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut len = [0u8; 4];
    match reader.read_exact(&mut len) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_MESSAGE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "message too large"));
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    Ok(Some(buf))
}

pub fn write_frame(writer: &mut impl Write, payload: &[u8]) -> io::Result<()> {
    if payload.len() > MAX_MESSAGE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "message too large"));
    }
    writer.write_all(&(payload.len() as u32).to_le_bytes())?;
    writer.write_all(payload)?;
    writer.flush()
}

pub fn write_json(writer: &mut impl Write, value: &impl Serialize) -> io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    write_frame(writer, &bytes)
}

/// Compares secrets without leaking the position of the first difference.
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip() {
        let mut buf = Vec::new();
        write_json(&mut buf, &BrowserRequest::Ping).unwrap();
        assert_eq!(&buf[..4], &(15u32).to_le_bytes());
        let mut r = buf.as_slice();
        let frame = read_frame(&mut r).unwrap().unwrap();
        assert_eq!(serde_json::from_slice::<BrowserRequest>(&frame).unwrap(), BrowserRequest::Ping);
        assert!(read_frame(&mut r).unwrap().is_none(), "EOF after the message");
    }

    #[test]
    fn rejects_oversized_frames() {
        let mut data = ((MAX_MESSAGE + 1) as u32).to_le_bytes().to_vec();
        data.extend(vec![b'x'; 16]);
        assert!(read_frame(&mut data.as_slice()).is_err());
    }

    #[test]
    fn parses_extension_messages_strictly() {
        let ok: BrowserRequest = serde_json::from_str(
            r#"{"type":"download","url":"https://e.com/a.zip","fileName":"a.zip","referrer":"https://e.com/","mime":"application/zip","size":10}"#,
        )
        .unwrap();
        assert!(ok.validate().is_ok());
        assert!(serde_json::from_str::<BrowserRequest>(r#"{"type":"download","url":"https://e.com","cookies":"x"}"#).is_err());
        assert!(serde_json::from_str::<BrowserRequest>(r#"{"type":"download","url":"https://e.com","cookies":[{"name":"a","value":"b","domain":"e.com","path":"/","extra":1}]}"#).is_err());
        assert!(serde_json::from_str::<BrowserRequest>(r#"{"type":"exec","cmd":"calc"}"#).is_err());
    }

    #[test]
    fn validates_urls_and_referrers() {
        let bad_url = BrowserRequest::Download { url: "file:///C:/Windows/win.ini".into(), file_name: None, referrer: None, mime: None, size: None, cookies: None };
        assert!(bad_url.validate().is_err());
        let bad_ref = BrowserRequest::Download { url: "https://e.com/a".into(), file_name: None, referrer: Some("javascript:alert(1)".into()), mime: None, size: None, cookies: None };
        assert!(bad_ref.validate().is_err());
        assert!(BrowserRequest::Media { url: "chrome://settings".into(), page_title: None, headers: None, cookies: None }.validate().is_err());
    }

    #[test]
    fn parses_panel_messages() {
        let f: BrowserRequest = serde_json::from_str(r#"{"type":"formats","url":"https://www.youtube.com/watch?v=x"}"#).unwrap();
        assert!(f.validate().is_ok());
        let d: BrowserRequest = serde_json::from_str(
            r#"{"type":"mediaDownload","url":"https://e.com/v","title":"Clip","options":{"mode":"video","maxHeight":720,"container":"mp4"}}"#,
        )
        .unwrap();
        assert!(d.validate().is_ok());
        // Options are validated too, so a format id can't smuggle in flags.
        let bad: BrowserRequest = serde_json::from_str(
            r#"{"type":"mediaDownload","url":"https://e.com/v","options":{"mode":"format","formatId":"--exec calc","container":"mp4"}}"#,
        )
        .unwrap();
        assert!(bad.validate().is_err());
        assert!(serde_json::from_str::<BrowserRequest>(r#"{"type":"formats","url":"https://e.com","extra":1}"#).is_err());
    }

    #[test]
    fn carries_sign_in_cookies_for_the_same_site_only() {
        let mut f: BrowserRequest = serde_json::from_str(
            r#"{"type":"formats","url":"https://www.youtube.com/watch?v=x","cookies":[{"name":"SID","value":"abc","domain":".youtube.com","path":"/","secure":true,"httpOnly":true,"hostOnly":false,"expires":1900000000}]}"#,
        )
        .unwrap();
        assert!(f.validate().is_ok());
        let (url, cookies) = f.take_cookies().unwrap();
        assert_eq!((url.as_str(), cookies[0].name.as_str()), ("https://www.youtube.com/watch?v=x", "SID"));
        assert!(f.take_cookies().is_none(), "taken once");
        let other: BrowserRequest = serde_json::from_str(
            r#"{"type":"formats","url":"https://www.youtube.com/watch?v=x","cookies":[{"name":"SID","value":"abc","domain":".google.com","path":"/"}]}"#,
        )
        .unwrap();
        assert!(other.validate().is_err(), "cookies for another site are refused");
    }

    #[test]
    fn player_streams_carry_the_page_as_referrer() {
        let f: BrowserRequest = serde_json::from_str(
            r#"{"type":"formats","url":"https://cdn.example.net/hls/master.m3u8","referrer":"https://videos.example.com/watch/1"}"#,
        )
        .unwrap();
        assert!(f.validate().is_ok());
        let bad: BrowserRequest =
            serde_json::from_str(r#"{"type":"formats","url":"https://cdn.example.net/a.m3u8","referrer":"javascript:alert(1)"}"#).unwrap();
        assert!(bad.validate().is_err());
        let r = BrowserResponse::media_error(crate::media::output::UNSUPPORTED_SITE.into(), false);
        assert!(r.unsupported && !r.sign_in);
    }

    #[test]
    fn captured_headers_are_validated_and_filtered() {
        let mut f: BrowserRequest = serde_json::from_str(
            r#"{"type":"formats","url":"https://cdn.example.net/v.m3u8","headers":{"Origin":"https://site.example","Cookie":"s=1","X-Session":"abc"}}"#,
        )
        .unwrap();
        assert!(f.validate().is_ok());
        let (url, kept) = f.take_headers().unwrap();
        assert_eq!(url, "https://cdn.example.net/v.m3u8");
        assert_eq!(kept, [("Origin".to_string(), "https://site.example".to_string()), ("X-Session".to_string(), "abc".to_string())]);
        let bad: BrowserRequest =
            serde_json::from_str(r#"{"type":"formats","url":"https://cdn.example.net/v.m3u8","headers":{"Origin":"a\nb"}}"#).unwrap();
        assert!(bad.validate().is_err());
    }

    #[test]
    fn sign_in_errors_tell_the_panel_what_helps() {
        use crate::media::output::{SIGN_IN_NEEDED, SIGN_IN_REFUSED};
        assert!(BrowserResponse::media_error(SIGN_IN_NEEDED.into(), false).sign_in);
        let refused = BrowserResponse::media_error(SIGN_IN_NEEDED.into(), true);
        assert!(!refused.sign_in);
        assert_eq!(refused.error.as_deref(), Some(SIGN_IN_REFUSED));
        assert!(!BrowserResponse::media_error("other".into(), false).sign_in);
    }

    #[test]
    fn constant_time_comparison() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "abcd"));
    }
}
