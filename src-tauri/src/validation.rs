//! Input validation for URLs, filenames and destination folders.
//!
//! These checks run in Rust regardless of what the frontend already
//! validated — the webview is never trusted to enforce them.

use std::path::{Path, PathBuf};

use url::Url;

use crate::error::{AppError, AppResult};

pub const MAX_URL_LEN: usize = 8 * 1024;
const MAX_FILE_NAME_LEN: usize = 200;
const FALLBACK_FILE_NAME: &str = "download";

/// Device names Windows reserves regardless of extension.
const RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Parses and validates a download URL. Only `http` and `https` with a host
/// are accepted; embedded credentials are rejected so they never reach the
/// database or logs.
pub fn validate_url(raw: &str) -> AppResult<Url> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(AppError::Validation("Enter a URL to download.".into()));
    }
    if trimmed.len() > MAX_URL_LEN {
        return Err(AppError::Validation("The URL is too long.".into()));
    }
    let url = Url::parse(trimmed)
        .map_err(|_| AppError::Validation("That doesn't look like a valid URL.".into()))?;
    match url.scheme() {
        "http" | "https" => {}
        other => {
            return Err(AppError::Validation(format!(
                "Unsupported protocol \"{other}\". Only HTTP and HTTPS links are supported."
            )));
        }
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(AppError::Validation("The URL has no host name.".into()));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(AppError::Validation(
            "URLs containing a username or password are not supported.".into(),
        ));
    }
    Ok(url)
}

/// Derives a safe filename from the last path segment of a URL.
pub fn file_name_from_url(url: &Url) -> String {
    let segment = url
        .path_segments()
        .and_then(|mut segments| segments.rfind(|s| !s.is_empty()))
        .unwrap_or_default();
    let decoded = percent_decode(segment);
    sanitize_file_name(&decoded)
}

/// Makes a filename safe to create on Windows: strips path separators and
/// reserved characters, control characters, trailing dots/spaces and
/// reserved device names, and caps the length while keeping the extension.
pub fn sanitize_file_name(raw: &str) -> String {
    let mut name: String = raw
        .chars()
        // Invisible formatting characters can disguise an extension
        // ("photo\u{202E}gpj.exe" displays as "photoexe.jpg"), so they go.
        .filter(|c| !is_invisible_format(*c))
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();

    name = name.trim().trim_end_matches(['.', ' ']).to_string();
    // A name made only of dots ("..") would refer to a parent directory.
    if name.chars().all(|c| c == '.' || c == '_') {
        name.clear();
    }
    if name.is_empty() {
        return FALLBACK_FILE_NAME.to_string();
    }

    let stem = name.split('.').next().unwrap_or_default().trim_end();
    if RESERVED_NAMES.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
        name.insert(0, '_');
    }

    truncate_preserving_extension(&name, MAX_FILE_NAME_LEN)
}

/// Bidirectional overrides and isolates, zero-width characters and the BOM.
fn is_invisible_format(c: char) -> bool {
    matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2069}' | '\u{FEFF}' | '\u{061C}')
}

/// Validates a user-chosen filename and returns its sanitized form.
pub fn validate_file_name(raw: &str) -> AppResult<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(AppError::Validation("Enter a file name.".into()));
    }
    if trimmed.contains(['/', '\\']) {
        return Err(AppError::Validation(
            "The file name can't contain folder separators.".into(),
        ));
    }
    Ok(sanitize_file_name(trimmed))
}

/// Validates an optional SHA-256 checksum and returns it as lowercase hex.
pub fn validate_sha256(raw: &str) -> AppResult<Option<String>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.len() != 64 || !trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(AppError::Validation(
            "A SHA-256 checksum is 64 hexadecimal characters.".into(),
        ));
    }
    Ok(Some(trimmed.to_ascii_lowercase()))
}

/// Validates that a destination folder is an absolute path to an existing
/// directory and returns its canonical form.
pub fn validate_destination_dir(raw: &str) -> AppResult<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(AppError::Validation("Choose a destination folder.".into()));
    }
    let path = Path::new(trimmed);
    if !path.is_absolute() {
        return Err(AppError::Validation(
            "The destination folder must be a full path.".into(),
        ));
    }
    if !path.is_dir() {
        return Err(AppError::Validation(
            "The destination folder doesn't exist.".into(),
        ));
    }
    let canonical = dunce_canonicalize(path)?;
    Ok(canonical)
}

/// `std::fs::canonicalize` returns `\\?\C:\...` on Windows; strip the
/// verbatim prefix for display when the path is a plain drive path.
fn dunce_canonicalize(path: &Path) -> AppResult<PathBuf> {
    let canonical = std::fs::canonicalize(path)?;
    #[cfg(windows)]
    {
        let s = canonical.to_string_lossy();
        if let Some(rest) = s.strip_prefix(r"\\?\")
            && !rest.starts_with("UNC\\")
        {
            return Ok(PathBuf::from(rest));
        }
    }
    Ok(canonical)
}

fn truncate_preserving_extension(name: &str, max_chars: usize) -> String {
    if name.chars().count() <= max_chars {
        return name.to_string();
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 && name.len() - i <= 16 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    let keep = max_chars.saturating_sub(ext.chars().count());
    let mut out: String = stem.chars().take(keep).collect();
    out = out.trim_end_matches(['.', ' ']).to_string();
    out.push_str(ext);
    out
}

/// Minimal percent-decoding for URL path segments; invalid UTF-8 is replaced.
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            out.push(h << 4 | l);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_invisible_characters_that_disguise_extensions() {
        assert_eq!(sanitize_file_name("photo\u{202E}gpj.exe"), "photogpj.exe");
        assert_eq!(sanitize_file_name("re\u{200B}port\u{FEFF}.pdf"), "report.pdf");
        assert_eq!(sanitize_file_name("\u{2066}\u{2069}"), "download");
    }

    #[test]
    fn accepts_http_and_https() {
        assert!(validate_url("https://example.com/file.zip").is_ok());
        assert!(validate_url("  http://example.com/a  ").is_ok());
    }

    #[test]
    fn rejects_other_schemes_and_garbage() {
        for bad in ["", "ftp://example.com/x", "file:///C:/Windows/win.ini", "javascript:alert(1)", "not a url"] {
            assert!(validate_url(bad).is_err(), "{bad} should be rejected");
        }
    }

    #[test]
    fn rejects_embedded_credentials() {
        assert!(validate_url("https://user:pass@example.com/file").is_err());
    }

    #[test]
    fn derives_file_name_from_url() {
        let url = validate_url("https://example.com/path/My%20File.zip?x=1").unwrap();
        assert_eq!(file_name_from_url(&url), "My File.zip");
        let root = validate_url("https://example.com/").unwrap();
        assert_eq!(file_name_from_url(&root), "download");
        let trailing = validate_url("https://example.com/dir/").unwrap();
        assert_eq!(file_name_from_url(&trailing), "dir");
    }

    #[test]
    fn sanitizes_dangerous_names() {
        assert_eq!(sanitize_file_name("..\\..\\evil.exe"), ".._.._evil.exe");
        assert_eq!(sanitize_file_name(".."), "download");
        assert_eq!(sanitize_file_name("a<b>c:d.txt"), "a_b_c_d.txt");
        assert_eq!(sanitize_file_name("CON.txt"), "_CON.txt");
        assert_eq!(sanitize_file_name("report.pdf. . "), "report.pdf");
        assert_eq!(sanitize_file_name("tab\there"), "tab_here");
    }

    #[test]
    fn caps_long_names_and_keeps_extension() {
        let long = format!("{}.mp4", "a".repeat(400));
        let out = sanitize_file_name(&long);
        assert_eq!(out.chars().count(), MAX_FILE_NAME_LEN);
        assert!(out.ends_with(".mp4"));
    }

    #[test]
    fn user_file_names_cannot_contain_separators() {
        assert!(validate_file_name("dir/file.txt").is_err());
        assert!(validate_file_name("dir\\file.txt").is_err());
        assert_eq!(validate_file_name(" ok.txt ").unwrap(), "ok.txt");
    }

    #[test]
    fn sha256_must_be_64_hex_chars() {
        assert_eq!(validate_sha256("  ").unwrap(), None);
        let upper = "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD";
        assert_eq!(validate_sha256(upper).unwrap().unwrap(), upper.to_ascii_lowercase());
        assert!(validate_sha256("abc").is_err());
        assert!(validate_sha256(&"g".repeat(64)).is_err());
    }

    #[test]
    fn destination_must_be_existing_absolute_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(validate_destination_dir(dir.path().to_str().unwrap()).is_ok());
        assert!(validate_destination_dir("relative\\path").is_err());
        let missing = dir.path().join("missing");
        assert!(validate_destination_dir(missing.to_str().unwrap()).is_err());
    }
}
