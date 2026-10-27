//! What the last format check found for a video, kept briefly so its download
//! can start from it (yt-dlp's `--load-info-json`) instead of asking the site
//! again, which takes several seconds on YouTube.
//!
//! Kept in memory only, used once, and forgotten after [`TTL`]: the stream
//! addresses inside expire. The JSON can hold sign-in cookies, so it reaches
//! disk only as a temporary file for yt-dlp, deleted when the run ends.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::error::{AppError, AppResult};

/// Long enough to pick a quality and confirm the download; well within the
/// lifetime of YouTube's stream addresses.
pub const TTL: Duration = Duration::from_secs(20 * 60);
/// Most videos remembered at once; the oldest go first.
const MAX_ENTRIES: usize = 16;

/// Recent format checks, by video address.
#[derive(Default)]
pub struct InfoCache {
    entries: Mutex<HashMap<String, (Vec<u8>, Instant)>>,
}

impl InfoCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn remember(&self, url: &str, json: Vec<u8>) {
        let Ok(mut entries) = self.entries.lock() else { return };
        entries.retain(|_, (_, at)| at.elapsed() < TTL);
        if entries.len() >= MAX_ENTRIES
            && let Some(oldest) = entries.iter().min_by_key(|(_, (_, at))| *at).map(|(k, _)| k.clone())
        {
            entries.remove(&oldest);
        }
        entries.insert(url.to_owned(), (json, Instant::now()));
    }

    /// The check for `url` if it's recent, removed so it's used only once
    /// (a retry or a later resume asks the site again).
    pub fn take(&self, url: &str) -> Option<Vec<u8>> {
        let mut entries = self.entries.lock().ok()?;
        entries.remove(url).filter(|(_, at)| at.elapsed() < TTL).map(|(json, _)| json)
    }
}

/// A temporary copy of a format check for yt-dlp, deleted when dropped.
pub struct InfoFile {
    path: PathBuf,
}

impl InfoFile {
    pub fn write(dir: &Path, json: &[u8]) -> AppResult<Self> {
        let path = dir.join(format!("bdm-info-{}.json", uuid::Uuid::new_v4().simple()));
        std::fs::write(&path, json).map_err(|e| AppError::Io(format!("couldn't prepare the video details: {e}")))?;
        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for InfoFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn used_once() {
        let cache = InfoCache::new();
        cache.remember("https://v.example/1", b"{}".to_vec());
        assert_eq!(cache.take("https://v.example/2"), None);
        assert_eq!(cache.take("https://v.example/1").as_deref(), Some(&b"{}"[..]));
        assert_eq!(cache.take("https://v.example/1"), None, "a second download asks the site again");
    }

    #[test]
    fn keeps_only_the_newest() {
        let cache = InfoCache::new();
        for i in 0..=MAX_ENTRIES {
            cache.remember(&format!("https://v.example/{i}"), vec![]);
        }
        assert_eq!(cache.take("https://v.example/0"), None, "the oldest went first");
        assert!(cache.take(&format!("https://v.example/{MAX_ENTRIES}")).is_some());
    }

    #[test]
    fn file_is_deleted_when_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let file = InfoFile::write(dir.path(), b"{\"id\":\"x\"}").unwrap();
        let path = file.path().to_owned();
        assert!(path.is_file());
        drop(file);
        assert!(!path.exists());
    }
}
