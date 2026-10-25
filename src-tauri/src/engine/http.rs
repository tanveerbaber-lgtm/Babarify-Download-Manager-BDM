//! HTTP client setup and remote-file probing.

use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{
    ACCEPT_ENCODING, CONTENT_LENGTH, CONTENT_RANGE, ETAG, HeaderMap, LAST_MODIFIED, RANGE, REFERER,
    RETRY_AFTER,
};
use reqwest::{Client, Response, StatusCode, redirect};

use super::EngineConfig;
use super::error::{EngineError, EngineResult};
use crate::net_guard::{GuardResolver, NetGuard};

const MAX_REDIRECTS: usize = 10;

/// Why a redirect may not be followed, if it may not.
pub fn redirect_refusal(previous: &[url::Url], next: &url::Url, guard: &NetGuard) -> Option<Box<dyn std::error::Error + Send + Sync>> {
    if previous.len() >= MAX_REDIRECTS {
        return Some("too many redirects".into());
    }
    if !matches!(next.scheme(), "http" | "https") {
        return Some("redirect to an unsupported protocol".into());
    }
    // An encrypted download must not quietly continue over plain HTTP.
    if next.scheme() == "http" && previous.iter().any(|u| u.scheme() == "https") {
        return Some("the server redirected from HTTPS to unencrypted HTTP".into());
    }
    guard.check_literal(next).err().map(|b| Box::new(b) as _)
}

/// The HTTP client every download uses; `guard` decides which addresses it may reach.
pub fn build_client(cfg: &EngineConfig, guard: Arc<NetGuard>) -> reqwest::Result<Client> {
    let mut headers = HeaderMap::new();
    // Byte offsets must refer to the file itself, never to a compressed encoding.
    headers.insert(ACCEPT_ENCODING, "identity".parse().expect("static header"));

    Client::builder()
        .user_agent(cfg.user_agent.clone())
        .default_headers(headers)
        .connect_timeout(cfg.connect_timeout)
        .read_timeout(cfg.read_timeout)
        .dns_resolver(GuardResolver(guard.clone()))
        .redirect(redirect::Policy::custom(move |attempt| {
            match redirect_refusal(attempt.previous(), attempt.url(), &guard) {
                Some(reason) => attempt.error(reason),
                None => attempt.follow(),
            }
        }))
        .build()
}

/// What the server told us about the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFile {
    pub final_url: String,
    pub total_bytes: Option<u64>,
    pub resumable: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    /// Sent as `Referer` on every request for this file.
    pub referrer: Option<String>,
}

impl RemoteFile {
    /// A GET for this file, with the referrer if there is one.
    pub fn get(&self, client: &Client) -> reqwest::RequestBuilder {
        let referrer = referrer_for(self.referrer.as_deref(), &self.final_url);
        with_referrer(client.get(&self.final_url), referrer.as_deref())
    }

    /// Validator for `If-Range`: a strong ETag, else Last-Modified.
    pub fn if_range(&self) -> Option<&str> {
        self.etag
            .as_deref()
            .filter(|e| !e.starts_with("W/"))
            .or(self.last_modified.as_deref())
    }
}

/// Asks for the first byte only. A `206` reveals range support and the full
/// size in one round trip; a `200` means ranges are not supported.
pub async fn probe(client: &Client, url: &str, referrer: Option<&str>) -> EngineResult<RemoteFile> {
    let sent = referrer_for(referrer, url);
    let resp = with_referrer(client.get(url), sent.as_deref())
        .header(RANGE, "bytes=0-0")
        .send()
        .await
        .map_err(EngineError::from_reqwest)?;

    let status = resp.status();
    let headers = resp.headers().clone();
    let final_url = resp.url().to_string();
    drop(resp);

    let etag = header_str(&headers, ETAG);
    let last_modified = header_str(&headers, LAST_MODIFIED);

    let (total_bytes, resumable) = match status {
        StatusCode::PARTIAL_CONTENT => {
            let total = header_str(&headers, CONTENT_RANGE).and_then(|v| parse_content_range(&v)).and_then(|r| r.total);
            (total, total.is_some())
        }
        StatusCode::OK => {
            let len = header_str(&headers, CONTENT_LENGTH).and_then(|v| v.parse().ok());
            // The server ignored our Range header, so it can't resume.
            (len, false)
        }
        // An empty file: there is no first byte to return.
        StatusCode::RANGE_NOT_SATISFIABLE
            if header_str(&headers, CONTENT_RANGE).as_deref() == Some("bytes */0") =>
        {
            (Some(0), false)
        }
        other => return Err(http_error(other, &headers)),
    };

    Ok(RemoteFile { final_url, total_bytes, resumable, etag, last_modified, referrer: referrer.map(str::to_owned) })
}

/// The `Referer` a browser would send (its default "strict-origin-when-cross-origin"
/// policy): the full page address only to the same site, just the origin to
/// other sites, and nothing from an HTTPS page to an HTTP server. Page
/// addresses can carry tokens that a file host has no business seeing.
pub fn referrer_for(referrer: Option<&str>, target: &str) -> Option<String> {
    let page = url::Url::parse(referrer?).ok()?;
    let target = url::Url::parse(target).ok()?;
    if page.scheme() == "https" && target.scheme() == "http" {
        return None;
    }
    if page.origin() == target.origin() {
        let mut full = page;
        full.set_fragment(None);
        return Some(full.to_string());
    }
    Some(format!("{}/", page.origin().ascii_serialization()))
}

fn with_referrer(req: reqwest::RequestBuilder, referrer: Option<&str>) -> reqwest::RequestBuilder {
    match referrer {
        Some(r) if !r.is_empty() => req.header(REFERER, r),
        _ => req,
    }
}

pub fn http_error(status: StatusCode, headers: &HeaderMap) -> EngineError {
    let retry_after = header_str(headers, RETRY_AFTER)
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(|s| Duration::from_secs(s.min(60)));
    EngineError::Http { status: status.as_u16(), retry_after }
}

/// Fails with a classified error unless the status is 2xx.
pub fn ensure_success(resp: &Response) -> EngineResult<()> {
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(http_error(resp.status(), resp.headers()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContentRange {
    pub start: u64,
    pub end: u64,
    pub total: Option<u64>,
}

/// Parses `bytes START-END/TOTAL` (TOTAL may be `*`).
pub fn parse_content_range(value: &str) -> Option<ContentRange> {
    let rest = value.trim().strip_prefix("bytes ")?;
    let (range, total) = rest.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    let start = start.trim().parse().ok()?;
    let end = end.trim().parse().ok()?;
    if end < start {
        return None;
    }
    let total = match total.trim() {
        "*" => None,
        t => Some(t.parse().ok()?),
    };
    Some(ContentRange { start, end, total })
}

pub fn header_str(headers: &HeaderMap, name: reqwest::header::HeaderName) -> Option<String> {
    headers.get(name).and_then(|v| v.to_str().ok()).map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_content_range() {
        assert_eq!(
            parse_content_range("bytes 0-0/1234"),
            Some(ContentRange { start: 0, end: 0, total: Some(1234) })
        );
        assert_eq!(
            parse_content_range("bytes 100-199/*"),
            Some(ContentRange { start: 100, end: 199, total: None })
        );
        assert_eq!(parse_content_range("bytes 5-1/10"), None);
        assert_eq!(parse_content_range("items 0-1/2"), None);
    }

    #[test]
    fn if_range_prefers_strong_etag() {
        let mut r = RemoteFile {
            final_url: String::new(),
            total_bytes: Some(1),
            resumable: true,
            etag: Some("\"abc\"".into()),
            last_modified: Some("Tue, 01 Oct 2026 00:00:00 GMT".into()),
            referrer: None,
        };
        assert_eq!(r.if_range(), Some("\"abc\""));
        r.etag = Some("W/\"abc\"".into());
        assert_eq!(r.if_range(), Some("Tue, 01 Oct 2026 00:00:00 GMT"));
    }
}
