//! Persistence for user settings as JSON values in a key/value table.
//!
//! Missing or unreadable keys fall back to defaults, so adding a setting
//! never requires a migration.

use rusqlite::{Connection, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::db::now_ms;
use crate::error::{AppError, AppResult};
use crate::models::{DuplicateAction, Settings, SettingsPatch, Theme, WhenDone};
use crate::validation::validate_destination_dir;

pub const MIN_CONCURRENT: u32 = 1;
pub const MAX_CONCURRENT: u32 = 16;
pub const MIN_SEGMENTS: u32 = 1;
pub const MAX_SEGMENTS: u32 = 16;

const KEY_THEME: &str = "theme";
const KEY_DOWNLOAD_DIR: &str = "default_download_dir";
const KEY_MAX_CONCURRENT: &str = "max_concurrent_downloads";
const KEY_SEGMENTS: &str = "segments_per_download";
const KEY_CONFIRM_REMOVE: &str = "confirm_before_remove";
const KEY_AUTO_UPDATE_YT_DLP: &str = "auto_update_yt_dlp";
const KEY_SHOW_ON_BROWSER_ADD: &str = "show_on_browser_add";
const KEY_SPEED_LIMIT: &str = "speed_limit_kbps";
const KEY_MAX_RETRIES: &str = "max_retries";
const KEY_AUTO_RETRY_COUNT: &str = "auto_retry_count";
const KEY_AUTO_RETRY_DELAY: &str = "auto_retry_delay_minutes";
const KEY_SORT_CATEGORIES: &str = "sort_into_categories";
const KEY_NOTIFY_COMPLETE: &str = "notify_on_complete";
const KEY_NOTIFY_FAILURE: &str = "notify_on_failure";
const KEY_WHEN_ALL_DONE: &str = "when_all_done";
const KEY_DUPLICATE_ACTION: &str = "duplicate_action";

pub const MAX_SPEED_LIMIT_KBPS: u32 = 10_000_000;
pub const MAX_RETRIES_LIMIT: u32 = 10;
pub const MAX_AUTO_RETRY_DELAY: u32 = 24 * 60;

/// Loads settings, using `fallback_download_dir` when none has been saved.
pub fn load(conn: &Connection, fallback_download_dir: &str) -> AppResult<Settings> {
    Ok(Settings {
        theme: read(conn, KEY_THEME)?.unwrap_or(Theme::System),
        default_download_dir: read(conn, KEY_DOWNLOAD_DIR)?
            .unwrap_or_else(|| fallback_download_dir.to_string()),
        max_concurrent_downloads: read(conn, KEY_MAX_CONCURRENT)?
            .filter(|n| (MIN_CONCURRENT..=MAX_CONCURRENT).contains(n))
            .unwrap_or(3),
        segments_per_download: read(conn, KEY_SEGMENTS)?
            .filter(|n| (MIN_SEGMENTS..=MAX_SEGMENTS).contains(n))
            .unwrap_or(8),
        confirm_before_remove: read(conn, KEY_CONFIRM_REMOVE)?.unwrap_or(true),
        auto_update_yt_dlp: read(conn, KEY_AUTO_UPDATE_YT_DLP)?.unwrap_or(true),
        show_on_browser_add: read(conn, KEY_SHOW_ON_BROWSER_ADD)?.unwrap_or(true),
        speed_limit_kbps: read(conn, KEY_SPEED_LIMIT)?.filter(|n| *n <= MAX_SPEED_LIMIT_KBPS).unwrap_or(0),
        max_retries: read(conn, KEY_MAX_RETRIES)?.filter(|n| *n <= MAX_RETRIES_LIMIT).unwrap_or(5),
        auto_retry_count: read(conn, KEY_AUTO_RETRY_COUNT)?.filter(|n| *n <= MAX_RETRIES_LIMIT).unwrap_or(2),
        auto_retry_delay_minutes: read(conn, KEY_AUTO_RETRY_DELAY)?
            .filter(|n| (1..=MAX_AUTO_RETRY_DELAY).contains(n))
            .unwrap_or(5),
        sort_into_categories: read(conn, KEY_SORT_CATEGORIES)?.unwrap_or(true),
        notify_on_complete: read(conn, KEY_NOTIFY_COMPLETE)?.unwrap_or(true),
        notify_on_failure: read(conn, KEY_NOTIFY_FAILURE)?.unwrap_or(true),
        when_all_done: read(conn, KEY_WHEN_ALL_DONE)?.unwrap_or(WhenDone::Nothing),
        duplicate_action: read(conn, KEY_DUPLICATE_ACTION)?.unwrap_or(DuplicateAction::Add),
    })
}

/// Validates and applies a partial update atomically.
pub fn apply_patch(conn: &mut Connection, patch: &SettingsPatch) -> AppResult<()> {
    let download_dir = patch
        .default_download_dir
        .as_deref()
        .map(validate_destination_dir)
        .transpose()?
        .map(|p| p.to_string_lossy().into_owned());

    if let Some(n) = patch.max_concurrent_downloads
        && !(MIN_CONCURRENT..=MAX_CONCURRENT).contains(&n)
    {
        return Err(AppError::Validation(format!(
            "Concurrent downloads must be between {MIN_CONCURRENT} and {MAX_CONCURRENT}."
        )));
    }

    if let Some(n) = patch.segments_per_download
        && !(MIN_SEGMENTS..=MAX_SEGMENTS).contains(&n)
    {
        return Err(AppError::Validation(format!(
            "Connections per download must be between {MIN_SEGMENTS} and {MAX_SEGMENTS}."
        )));
    }

    let range = |v: Option<u32>, lo: u32, hi: u32, what: &str| -> AppResult<()> {
        match v {
            Some(n) if !(lo..=hi).contains(&n) => Err(AppError::Validation(format!("{what} must be between {lo} and {hi}."))),
            _ => Ok(()),
        }
    };
    range(patch.speed_limit_kbps, 0, MAX_SPEED_LIMIT_KBPS, "The speed limit")?;
    range(patch.max_retries, 0, MAX_RETRIES_LIMIT, "Retries")?;
    range(patch.auto_retry_count, 0, MAX_RETRIES_LIMIT, "Automatic retries")?;
    range(patch.auto_retry_delay_minutes, 1, MAX_AUTO_RETRY_DELAY, "The retry delay")?;

    let tx = conn.transaction()?;
    if let Some(theme) = patch.theme {
        write(&tx, KEY_THEME, &theme)?;
    }
    if let Some(dir) = download_dir {
        write(&tx, KEY_DOWNLOAD_DIR, &dir)?;
    }
    if let Some(n) = patch.max_concurrent_downloads {
        write(&tx, KEY_MAX_CONCURRENT, &n)?;
    }
    if let Some(n) = patch.segments_per_download {
        write(&tx, KEY_SEGMENTS, &n)?;
    }
    if let Some(confirm) = patch.confirm_before_remove {
        write(&tx, KEY_CONFIRM_REMOVE, &confirm)?;
    }
    if let Some(auto) = patch.auto_update_yt_dlp {
        write(&tx, KEY_AUTO_UPDATE_YT_DLP, &auto)?;
    }
    if let Some(show) = patch.show_on_browser_add {
        write(&tx, KEY_SHOW_ON_BROWSER_ADD, &show)?;
    }
    if let Some(v) = patch.speed_limit_kbps {
        write(&tx, KEY_SPEED_LIMIT, &v)?;
    }
    if let Some(v) = patch.max_retries {
        write(&tx, KEY_MAX_RETRIES, &v)?;
    }
    if let Some(v) = patch.auto_retry_count {
        write(&tx, KEY_AUTO_RETRY_COUNT, &v)?;
    }
    if let Some(v) = patch.auto_retry_delay_minutes {
        write(&tx, KEY_AUTO_RETRY_DELAY, &v)?;
    }
    if let Some(v) = patch.sort_into_categories {
        write(&tx, KEY_SORT_CATEGORIES, &v)?;
    }
    if let Some(v) = patch.notify_on_complete {
        write(&tx, KEY_NOTIFY_COMPLETE, &v)?;
    }
    if let Some(v) = patch.notify_on_failure {
        write(&tx, KEY_NOTIFY_FAILURE, &v)?;
    }
    if let Some(v) = patch.when_all_done {
        write(&tx, KEY_WHEN_ALL_DONE, &v)?;
    }
    if let Some(v) = patch.duplicate_action {
        write(&tx, KEY_DUPLICATE_ACTION, &v)?;
    }
    tx.commit()?;
    Ok(())
}

fn read<T: DeserializeOwned>(conn: &Connection, key: &str) -> AppResult<Option<T>> {
    let mut stmt = conn.prepare_cached("SELECT value FROM settings WHERE key = ?1")?;
    let mut rows = stmt.query([key])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    let raw: String = row.get(0)?;
    // A corrupt value is treated as unset rather than blocking startup.
    Ok(serde_json::from_str(&raw).ok())
}

fn write<T: Serialize>(conn: &Connection, key: &str, value: &T) -> AppResult<()> {
    conn.execute(
        "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![key, serde_json::to_string(value)?, now_ms()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    #[test]
    fn defaults_when_empty() {
        let conn = open_in_memory().unwrap();
        let s = load(&conn, "C:\\Users\\me\\Downloads").unwrap();
        assert_eq!(s.theme, Theme::System);
        assert_eq!(s.default_download_dir, "C:\\Users\\me\\Downloads");
        assert_eq!(s.max_concurrent_downloads, 3);
        assert_eq!(s.segments_per_download, 8);
        assert!(s.confirm_before_remove);
    }

    #[test]
    fn patch_persists_values() {
        let mut conn = open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let patch = SettingsPatch {
            theme: Some(Theme::Dark),
            default_download_dir: Some(dir.path().to_string_lossy().into_owned()),
            max_concurrent_downloads: Some(5),
            segments_per_download: Some(4),
            confirm_before_remove: Some(false),
            auto_update_yt_dlp: Some(false),
            show_on_browser_add: Some(false),
            speed_limit_kbps: Some(512),
            max_retries: Some(3),
            auto_retry_count: Some(1),
            auto_retry_delay_minutes: Some(10),
            sort_into_categories: Some(false),
            notify_on_complete: Some(false),
            notify_on_failure: Some(false),
            when_all_done: Some(WhenDone::Sleep),
            duplicate_action: Some(DuplicateAction::Skip),
        };
        apply_patch(&mut conn, &patch).unwrap();
        let s = load(&conn, "unused").unwrap();
        assert_eq!(s.theme, Theme::Dark);
        assert_eq!(s.max_concurrent_downloads, 5);
        assert_eq!(s.segments_per_download, 4);
        assert_eq!(s.speed_limit_kbps, 512);
        assert_eq!(s.when_all_done, WhenDone::Sleep);
        assert_eq!(s.duplicate_action, DuplicateAction::Skip);
        assert!(!s.sort_into_categories);
        assert!(!s.confirm_before_remove);
        assert_ne!(s.default_download_dir, "unused");
    }

    #[test]
    fn invalid_patch_changes_nothing() {
        let mut conn = open_in_memory().unwrap();
        let patch = SettingsPatch {
            theme: Some(Theme::Light),
            max_concurrent_downloads: Some(99),
            ..Default::default()
        };
        assert!(apply_patch(&mut conn, &patch).is_err());
        assert_eq!(load(&conn, "x").unwrap().theme, Theme::System);
    }

    #[test]
    fn corrupt_value_falls_back_to_default() {
        let conn = open_in_memory().unwrap();
        conn.execute(
            "INSERT INTO settings (key, value, updated_at) VALUES ('theme', 'not json', 0)",
            [],
        )
        .unwrap();
        assert_eq!(load(&conn, "x").unwrap().theme, Theme::System);
    }
}
