//! Messages between the browser extension, the native host and the app.
//!
//! Browser ↔ host uses Chrome's native-messaging framing: a 4-byte
//! little-endian length followed by UTF-8 JSON. Host ↔ app uses the same
//! framing over a named pipe, with the request wrapped in a [`PipeRequest`]
//! that carries the per-user token.

use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
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
    },
    /// Open BDM's video/audio dialog for a page.
    #[serde(rename_all = "camelCase")]
    Media { url: String, page_title: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
}

impl BrowserResponse {
    pub fn ok() -> Self {
        Self { ok: true, app: None, version: None, error: None }
    }

    pub fn running(version: &str) -> Self {
        Self { app: Some("running".into()), version: Some(version.into()), ..Self::ok() }
    }

    pub fn stopped() -> Self {
        Self { app: Some("stopped".into()), ..Self::ok() }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self { ok: false, app: None, version: None, error: Some(message.into()) }
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
    /// Checks every field before anything acts on it. The app validates
    /// again when it creates the download.
    pub fn validate(&self) -> AppResult<()> {
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
            Self::Media { url, page_title } => {
                validation::validate_url(url)?;
                if page_title.as_deref().is_some_and(|t| t.chars().count() > 1000) {
                    return Err(AppError::Validation("Page title is too long.".into()));
                }
                Ok(())
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
        assert!(serde_json::from_str::<BrowserRequest>(r#"{"type":"exec","cmd":"calc"}"#).is_err());
    }

    #[test]
    fn validates_urls_and_referrers() {
        let bad_url = BrowserRequest::Download { url: "file:///C:/Windows/win.ini".into(), file_name: None, referrer: None, mime: None, size: None };
        assert!(bad_url.validate().is_err());
        let bad_ref = BrowserRequest::Download { url: "https://e.com/a".into(), file_name: None, referrer: Some("javascript:alert(1)".into()), mime: None, size: None };
        assert!(bad_ref.validate().is_err());
        assert!(BrowserRequest::Media { url: "chrome://settings".into(), page_title: None }.validate().is_err());
    }

    #[test]
    fn constant_time_comparison() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "abcd"));
    }
}
