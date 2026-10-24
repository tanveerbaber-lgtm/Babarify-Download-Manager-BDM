//! Batch download helpers: numbered URL patterns and URL lists.

use crate::error::{AppError, AppResult};
use crate::validation;

/// Most URLs one batch may add.
pub const MAX_BATCH: usize = 1000;

/// Expands `file[01-20].zip` or `img[a-f].png` into the matching URLs.
/// A leading zero in the start number keeps every number that wide. One
/// range per pattern; a pattern without a range is returned as is.
pub fn expand_pattern(pattern: &str) -> AppResult<Vec<String>> {
    let pattern = pattern.trim();
    let bad = || AppError::Validation("Use one range like [1-20], [01-20] or [a-z].".into());
    let Some(open) = pattern.find('[') else { return Ok(vec![pattern.to_string()]) };
    let close = pattern[open..].find(']').map(|i| open + i).ok_or_else(bad)?;
    let (prefix, spec, suffix) = (&pattern[..open], &pattern[open + 1..close], &pattern[close + 1..]);
    if suffix.contains('[') {
        return Err(bad());
    }
    let (a, b) = spec.split_once('-').ok_or_else(bad)?;
    let (a, b) = (a.trim(), b.trim());

    let items: Vec<String> = if let (Ok(x), Ok(y)) = (a.parse::<u64>(), b.parse::<u64>()) {
        if x > y || y - x >= MAX_BATCH as u64 {
            return Err(AppError::Validation(format!("A range can cover at most {MAX_BATCH} numbers, counting up.")));
        }
        let width = if a.len() > 1 && a.starts_with('0') { a.len() } else { 0 };
        (x..=y).map(|n| format!("{n:0width$}")).collect()
    } else {
        let (Some(x), Some(y)) = (single_letter(a), single_letter(b)) else { return Err(bad()) };
        if x > y || x.is_ascii_lowercase() != y.is_ascii_lowercase() {
            return Err(bad());
        }
        (x..=y).map(String::from).collect()
    };
    Ok(items.into_iter().map(|i| format!("{prefix}{i}{suffix}")).collect())
}

fn single_letter(s: &str) -> Option<char> {
    let mut chars = s.chars();
    let c = chars.next()?;
    (chars.next().is_none() && c.is_ascii_alphabetic()).then_some(c)
}

/// Pulls the http(s) links out of pasted text or a .txt file, in order,
/// without duplicates. Lines starting with `#` are comments.
pub fn extract_urls(text: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    text.lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#'))
        .flat_map(|l| l.split_whitespace())
        .map(|w| w.trim_matches(|c| matches!(c, '"' | '\'' | '<' | '>' | ',' | ';')))
        .filter(|w| w.starts_with("http://") || w.starts_with("https://"))
        .filter_map(|w| validation::validate_url(w).ok())
        .map(|u| u.to_string())
        .filter(|u| seen.insert(u.clone()))
        .take(MAX_BATCH)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_numbers_with_padding() {
        let urls = expand_pattern("https://e.com/file[01-03].zip").unwrap();
        assert_eq!(urls, ["https://e.com/file01.zip", "https://e.com/file02.zip", "https://e.com/file03.zip"]);
        assert_eq!(expand_pattern("https://e.com/p[8-10].jpg").unwrap()[2], "https://e.com/p10.jpg");
        assert_eq!(expand_pattern("https://e.com/p[008-010].jpg").unwrap()[0], "https://e.com/p008.jpg");
    }

    #[test]
    fn expands_letters_and_plain_urls() {
        assert_eq!(expand_pattern("https://e.com/[a-c].png").unwrap().len(), 3);
        assert_eq!(expand_pattern("https://e.com/x.zip").unwrap(), ["https://e.com/x.zip"]);
    }

    #[test]
    fn rejects_bad_ranges() {
        for p in ["https://e.com/[5-1].zip", "https://e.com/[1-5000].zip", "https://e.com/[a-Z].zip", "https://e.com/[1-2][3-4]", "https://e.com/[1-x]", "https://e.com/[12"] {
            assert!(expand_pattern(p).is_err(), "{p}");
        }
    }

    #[test]
    fn extracts_urls_from_text() {
        let text = "# my list\nhttps://e.com/a.zip\n  \"https://e.com/b.zip\", ftp://nope\nsee https://e.com/a.zip again\njavascript:alert(1)";
        assert_eq!(extract_urls(text), ["https://e.com/a.zip", "https://e.com/b.zip"]);
    }
}
