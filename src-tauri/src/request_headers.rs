//! Request headers the browser sent for a video a page played, captured by the
//! extension's media detection, so yt-dlp can ask for that video the same way.
//!
//! Only headers that describe the request are kept: User-Agent, Referer,
//! Origin, Accept, Accept-Language and the page's own `X-` headers. Cookies
//! and Authorization never come this way (cookies only through sign-in
//! downloads). Kept in memory only and forgotten after [`TTL`].

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::error::{AppError, AppResult};

pub const TTL: Duration = Duration::from_secs(12 * 60 * 60);
const MAX_HEADERS: usize = 32;
const MAX_VALUE: usize = 4096;
/// Most videos remembered at once; the oldest go first.
const MAX_ENTRIES: usize = 200;
const ALLOWED: [&str; 5] = ["user-agent", "referer", "origin", "accept", "accept-language"];

/// Validates headers from the browser and keeps the ones yt-dlp may send.
pub fn sanitize(headers: &BTreeMap<String, String>) -> AppResult<Vec<(String, String)>> {
    if headers.len() > MAX_HEADERS {
        return Err(AppError::Validation("Too many request headers.".into()));
    }
    let mut kept = Vec::new();
    for (name, value) in headers {
        if name.is_empty() || name.len() > 64 || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err(AppError::Validation("Invalid request header name.".into()));
        }
        if value.len() > MAX_VALUE || value.chars().any(char::is_control) {
            return Err(AppError::Validation("Invalid request header value.".into()));
        }
        let lower = name.to_ascii_lowercase();
        if ALLOWED.contains(&lower.as_str()) || (lower.starts_with("x-") && !lower.starts_with("x-forwarded")) {
            kept.push((name.clone(), value.clone()));
        }
    }
    Ok(kept)
}

/// Header name and value pairs, in the order they're sent.
pub type Headers = Vec<(String, String)>;

/// Headers captured this session, by video address.
#[derive(Default)]
pub struct HeaderStore {
    entries: Mutex<HashMap<String, (Headers, Instant)>>,
}

impl HeaderStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn remember(&self, url: &str, headers: Vec<(String, String)>) {
        let Ok(mut entries) = self.entries.lock() else { return };
        entries.retain(|_, (_, at)| at.elapsed() < TTL);
        if headers.is_empty() {
            entries.remove(url);
            return;
        }
        if entries.len() >= MAX_ENTRIES
            && let Some(oldest) = entries.iter().min_by_key(|(_, (_, at))| *at).map(|(k, _)| k.clone())
        {
            entries.remove(&oldest);
        }
        entries.insert(url.to_owned(), (headers, Instant::now()));
    }

    pub fn for_url(&self, url: &str) -> Vec<(String, String)> {
        let Ok(entries) = self.entries.lock() else { return Vec::new() };
        entries.get(url).filter(|(_, at)| at.elapsed() < TTL).map(|(h, _)| h.clone()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn keeps_only_request_describing_headers() {
        let kept = sanitize(&map(&[
            ("User-Agent", "Mozilla/5.0"),
            ("Referer", "https://site.example/watch/1"),
            ("Origin", "https://site.example"),
            ("X-Playback-Session-Id", "abc"),
            ("Cookie", "secret=1"),
            ("Authorization", "Bearer t"),
            ("Range", "bytes=0-"),
            ("X-Forwarded-For", "1.2.3.4"),
            ("sec-ch-ua", "\"Chrome\""),
        ]))
        .unwrap();
        let names: Vec<&str> = kept.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["Origin", "Referer", "User-Agent", "X-Playback-Session-Id"]);
    }

    #[test]
    fn rejects_malformed_headers() {
        assert!(sanitize(&map(&[("Bad Name", "x")])).is_err());
        assert!(sanitize(&map(&[("Referer", "a\r\nInjected: 1")])).is_err());
        let many: BTreeMap<String, String> = (0..40).map(|i| (format!("X-H{i}"), "v".to_string())).collect();
        assert!(sanitize(&many).is_err());
    }

    #[test]
    fn store_remembers_by_address() {
        let store = HeaderStore::new();
        store.remember("https://cdn.example/v.m3u8", vec![("Origin".into(), "https://site.example".into())]);
        assert_eq!(store.for_url("https://cdn.example/v.m3u8").len(), 1);
        assert!(store.for_url("https://cdn.example/other.m3u8").is_empty());
        store.remember("https://cdn.example/v.m3u8", vec![]);
        assert!(store.for_url("https://cdn.example/v.m3u8").is_empty(), "an empty set clears it");
    }
}
