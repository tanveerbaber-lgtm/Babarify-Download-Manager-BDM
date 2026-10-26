//! Browser sign-in cookies for sites the user allowed in the extension.
//!
//! The extension sends the cookies a site would get with each request for
//! that site. They are kept in memory only, never written to the database,
//! and forgotten after [`TTL`] or when BDM closes. yt-dlp gets them through a
//! temporary cookies file that is deleted as soon as it finishes; file
//! downloads send them only to hosts the cookie belongs to.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// How long cookies from the browser are used before a fresh request is needed.
pub const TTL: Duration = Duration::from_secs(12 * 60 * 60);
const MAX_COOKIES: usize = 300;
/// Total size of names and values in one request.
const MAX_TOTAL_BYTES: usize = 48 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserCookie {
    pub name: String,
    pub value: String,
    /// As the browser reports it, e.g. ".youtube.com" or "www.example.com".
    pub domain: String,
    pub path: String,
    #[serde(default)]
    pub secure: bool,
    #[serde(default)]
    pub http_only: bool,
    /// Sent only to `domain` itself, not its subdomains.
    #[serde(default)]
    pub host_only: bool,
    /// Unix seconds; `None` for a session cookie.
    #[serde(default)]
    pub expires: Option<i64>,
}

impl BrowserCookie {
    fn bare_domain(&self) -> &str {
        self.domain.trim_start_matches('.')
    }

    /// Whether a browser would send this cookie to `host`.
    fn matches_host(&self, host: &str) -> bool {
        let d = self.bare_domain();
        host == d || (!self.host_only && host.ends_with(&format!(".{d}")))
    }

    fn expired(&self, now: i64) -> bool {
        self.expires.is_some_and(|e| e > 0 && e <= now)
    }

    fn key(&self) -> CookieKey {
        (self.bare_domain().to_owned(), self.path.clone(), self.name.clone())
    }
}

fn has_control(s: &str) -> bool {
    s.chars().any(|c| c.is_control())
}

/// Checks cookies sent with a request for `url`: each must belong to that
/// site, and none may contain characters that could break a cookies file
/// or an HTTP header.
pub fn validate(cookies: &[BrowserCookie], url: &str) -> AppResult<()> {
    let bad = |m: &str| Err(AppError::Validation(m.into()));
    if cookies.len() > MAX_COOKIES {
        return bad("Too many cookies.");
    }
    if cookies.iter().map(|c| c.name.len() + c.value.len()).sum::<usize>() > MAX_TOTAL_BYTES {
        return bad("Cookies are too large.");
    }
    let host = host_of(url).ok_or_else(|| AppError::Validation("Invalid URL.".into()))?;
    for c in cookies {
        let d = c.bare_domain();
        if c.name.is_empty()
            || c.name.len() > 256
            || c.value.len() > 8192
            || has_control(&c.name)
            || has_control(&c.value)
            || c.name.contains(['=', ';', ' '])
            || c.value.contains(';')
        {
            return bad("Invalid cookie.");
        }
        if d.is_empty() || d.len() > 253 || !d.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '-') {
            return bad("Invalid cookie domain.");
        }
        if !c.path.starts_with('/') || c.path.len() > 1024 || c.path.chars().any(|ch| ch.is_control() || ch == ' ') {
            return bad("Invalid cookie path.");
        }
        // A request for one site can't plant cookies for another.
        if !c.matches_host(&host) {
            return bad("A cookie doesn't belong to this site.");
        }
    }
    Ok(())
}

fn host_of(url: &str) -> Option<String> {
    url::Url::parse(url).ok()?.host_str().map(|h| h.to_ascii_lowercase())
}

fn unix_now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// Domain, path and name: what makes a cookie unique.
type CookieKey = (String, String, String);

/// Cookies received from the browser this session.
#[derive(Default)]
pub struct CookieJar {
    entries: Mutex<HashMap<CookieKey, (BrowserCookie, Instant)>>,
}

impl CookieJar {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces what BDM knows for `url`'s site with the browser's current
    /// cookies (so signing out in the browser is picked up too).
    pub fn remember(&self, url: &str, cookies: Vec<BrowserCookie>) {
        let Some(host) = host_of(url) else { return };
        let Ok(mut entries) = self.entries.lock() else { return };
        entries.retain(|_, (c, _)| !c.matches_host(&host));
        let now = Instant::now();
        for c in cookies {
            entries.insert(c.key(), (c, now));
        }
    }

    /// Cookies a browser would send to `url`'s host (any path).
    pub fn for_url(&self, url: &str) -> Vec<BrowserCookie> {
        let Some(host) = host_of(url) else { return Vec::new() };
        let Ok(mut entries) = self.entries.lock() else { return Vec::new() };
        let now = unix_now();
        entries.retain(|_, (c, at)| at.elapsed() < TTL && !c.expired(now));
        let mut found: Vec<BrowserCookie> = entries.values().filter(|(c, _)| c.matches_host(&host)).map(|(c, _)| c.clone()).collect();
        found.sort_by(|a, b| b.path.len().cmp(&a.path.len()).then_with(|| a.name.cmp(&b.name)));
        found
    }

    /// A `Cookie` header for one request, following the browser's path and
    /// secure rules.
    pub fn header_for(&self, url: &str) -> Option<String> {
        let parsed = url::Url::parse(url).ok()?;
        let https = parsed.scheme() == "https";
        let path = parsed.path();
        let pairs: Vec<String> = self
            .for_url(url)
            .into_iter()
            .filter(|c| (https || !c.secure) && path_matches(&c.path, path))
            .map(|c| format!("{}={}", c.name, c.value))
            .collect();
        (!pairs.is_empty()).then(|| pairs.join("; "))
    }

    /// Writes the cookies for `url` to a temporary file for yt-dlp, if there are any.
    pub fn file_for(&self, url: &str) -> AppResult<Option<CookieFile>> {
        let cookies = self.for_url(url);
        if cookies.is_empty() {
            return Ok(None);
        }
        CookieFile::write(&std::env::temp_dir(), &cookies).map(Some)
    }
}

/// RFC 6265 path matching.
fn path_matches(cookie_path: &str, request_path: &str) -> bool {
    request_path == cookie_path
        || (request_path.starts_with(cookie_path)
            && (cookie_path.ends_with('/') || request_path[cookie_path.len()..].starts_with('/')))
}

/// The Netscape cookies-file format yt-dlp reads.
pub fn netscape(cookies: &[BrowserCookie]) -> String {
    let mut out = String::from("# Netscape HTTP Cookie File\n");
    for c in cookies {
        let flag = |b: bool| if b { "TRUE" } else { "FALSE" };
        let domain = if c.host_only { c.bare_domain().to_owned() } else { format!(".{}", c.bare_domain()) };
        let prefix = if c.http_only { "#HttpOnly_" } else { "" };
        out.push_str(&format!(
            "{prefix}{domain}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            flag(!c.host_only),
            c.path,
            flag(c.secure),
            c.expires.unwrap_or(0).max(0),
            c.name,
            c.value
        ));
    }
    out
}

/// A temporary cookies file, deleted when dropped.
pub struct CookieFile {
    path: PathBuf,
}

impl CookieFile {
    fn write(dir: &Path, cookies: &[BrowserCookie]) -> AppResult<Self> {
        let path = dir.join(format!("bdm-cookies-{}.txt", uuid::Uuid::new_v4().simple()));
        std::fs::write(&path, netscape(cookies)).map_err(|e| AppError::Io(format!("couldn't prepare sign-in cookies: {e}")))?;
        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for CookieFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cookie(name: &str, domain: &str) -> BrowserCookie {
        BrowserCookie {
            name: name.into(),
            value: "v".into(),
            domain: domain.into(),
            path: "/".into(),
            secure: true,
            http_only: false,
            host_only: !domain.starts_with('.'),
            expires: None,
        }
    }

    #[test]
    fn accepts_only_cookies_for_the_requested_site() {
        let url = "https://www.youtube.com/watch?v=x";
        assert!(validate(&[cookie("SID", ".youtube.com"), cookie("a", "www.youtube.com")], url).is_ok());
        assert!(validate(&[cookie("SID", ".google.com")], url).is_err(), "another site's cookie");
        assert!(validate(&[cookie("a", "youtube.com")], url).is_err(), "host-only cookie for the parent domain");
        let mut bad = cookie("a", ".youtube.com");
        bad.value = "x\ny".into();
        assert!(validate(&[bad], url).is_err(), "newline would break the cookies file");
        let mut bad = cookie("a", ".youtube.com");
        bad.path = "nope".into();
        assert!(validate(&[bad], url).is_err());
        assert!(validate(&vec![cookie("a", ".youtube.com"); MAX_COOKIES + 1], url).is_err());
    }

    #[test]
    fn jar_matches_hosts_paths_and_replaces_per_site() {
        let jar = CookieJar::new();
        let mut scoped = cookie("p", ".example.com");
        scoped.path = "/files".into();
        jar.remember("https://www.example.com/", vec![cookie("s", ".example.com"), scoped]);
        jar.remember("https://other.net/", vec![cookie("o", "other.net")]);

        assert_eq!(jar.header_for("https://dl.example.com/files/a.zip").as_deref(), Some("p=v; s=v"));
        assert_eq!(jar.header_for("https://dl.example.com/filesx").as_deref(), Some("s=v"), "path must match on a / boundary");
        assert_eq!(jar.header_for("http://dl.example.com/"), None, "secure cookies only over HTTPS");
        assert_eq!(jar.header_for("https://cdn.net/"), None);

        // A newer request for the site replaces its cookies (e.g. after signing out).
        jar.remember("https://www.example.com/", vec![]);
        assert_eq!(jar.header_for("https://dl.example.com/files/a.zip"), None);
        assert_eq!(jar.header_for("https://other.net/").as_deref(), Some("o=v"));
    }

    #[test]
    fn drops_expired_cookies() {
        let jar = CookieJar::new();
        let mut old = cookie("old", ".e.com");
        old.expires = Some(1);
        jar.remember("https://e.com/", vec![old, cookie("new", ".e.com")]);
        assert_eq!(jar.for_url("https://e.com/").len(), 1);
    }

    #[test]
    fn writes_a_netscape_file_and_deletes_it() {
        let mut http_only = cookie("SID", ".youtube.com");
        http_only.http_only = true;
        http_only.expires = Some(1_900_000_000);
        let text = netscape(&[http_only, cookie("a", "www.youtube.com")]);
        assert!(text.starts_with("# Netscape HTTP Cookie File\n"));
        assert!(text.contains("#HttpOnly_.youtube.com\tTRUE\t/\tTRUE\t1900000000\tSID\tv\n"));
        assert!(text.contains("www.youtube.com\tFALSE\t/\tTRUE\t0\ta\tv\n"));

        let jar = CookieJar::new();
        assert!(jar.file_for("https://e.com/").unwrap().is_none(), "no file without cookies");
        jar.remember("https://e.com/", vec![cookie("a", ".e.com")]);
        let file = jar.file_for("https://e.com/v").unwrap().unwrap();
        let path = file.path().to_path_buf();
        assert!(std::fs::read_to_string(&path).unwrap().contains("\ta\tv"));
        drop(file);
        assert!(!path.exists(), "deleted after use");
    }
}
