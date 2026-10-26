//! Persistence for download records and their segments.

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::db::now_ms;
use crate::error::{AppError, AppResult};
use crate::models::{Download, DownloadKind, DownloadStatus, MediaOptions, Segment};

const COLUMNS: &str = "id, url, file_name, destination_dir, status, total_bytes, \
                       downloaded_bytes, error_message, created_at, updated_at, completed_at, \
                       final_url, saved_path, resumable, expected_sha256, temp_path, etag, last_modified, \
                       kind, title, media_options, referrer, queue_id, priority, position, category, \
                       speed_limit, auto_retries, retry_at";

/// Fields needed to create a new pending record (already validated).
pub struct InsertDownload<'a> {
    pub url: &'a str,
    pub file_name: &'a str,
    pub destination_dir: &'a str,
    pub expected_sha256: Option<&'a str>,
    pub kind: DownloadKind,
    pub title: Option<&'a str>,
    pub media_options: Option<&'a MediaOptions>,
    pub referrer: Option<&'a str>,
    pub queue_id: &'a str,
    pub priority: i32,
    pub category: Option<&'a str>,
    pub speed_limit: Option<i64>,
}

/// What the engine learned from the server about the remote file.
pub struct RemoteInfo<'a> {
    pub final_url: &'a str,
    pub total_bytes: Option<u64>,
    pub resumable: bool,
    pub etag: Option<&'a str>,
    pub last_modified: Option<&'a str>,
}

pub fn insert(conn: &Connection, new: &InsertDownload<'_>) -> AppResult<Download> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = now_ms();
    let media_options = new.media_options.map(serde_json::to_string).transpose()?;
    conn.execute(
        "INSERT INTO downloads (id, url, file_name, destination_dir, status, expected_sha256,
                                kind, title, media_options, referrer, created_at, updated_at,
                                queue_id, priority, position, category, speed_limit)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?11, ?10, ?10, ?12, ?13, ?14, ?15, ?16)",
        params![
            id,
            new.url,
            new.file_name,
            new.destination_dir,
            DownloadStatus::Pending.as_str(),
            new.expected_sha256,
            new.kind.as_str(),
            new.title,
            media_options,
            now,
            new.referrer,
            new.queue_id,
            new.priority.clamp(-1, 1),
            next_position(conn)?,
            new.category,
            new.speed_limit.filter(|n| *n > 0)
        ],
    )?;
    require(conn, &id)
}

/// A position after every existing one (new downloads go to the end of their queue).
fn next_position(conn: &Connection) -> AppResult<i64> {
    let max: i64 = conn.query_row("SELECT COALESCE(MAX(position), 0) FROM downloads", [], |r| r.get(0))?;
    Ok(max.max(now_ms()) + 1)
}

pub fn get(conn: &Connection, id: &str) -> AppResult<Option<Download>> {
    let sql = format!("SELECT {COLUMNS} FROM downloads WHERE id = ?1");
    Ok(conn.query_row(&sql, [id], from_row).optional()?)
}

/// Like [`get`], but a missing record is an error.
pub fn require(conn: &Connection, id: &str) -> AppResult<Download> {
    get(conn, id)?.ok_or_else(|| AppError::NotFound("That download no longer exists.".into()))
}

pub fn list(conn: &Connection) -> AppResult<Vec<Download>> {
    let sql = format!("SELECT {COLUMNS} FROM downloads ORDER BY created_at DESC, id");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Queued downloads in start order: high priority first, then queue position.
pub fn queued_in_order(conn: &Connection) -> AppResult<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT id, queue_id FROM downloads WHERE status = 'queued'
         ORDER BY priority DESC, position, created_at, id",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Failed downloads whose automatic retry is due.
pub fn due_retries(conn: &Connection, now: i64) -> AppResult<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM downloads WHERE status = 'failed' AND retry_at IS NOT NULL AND retry_at <= ?1
         ORDER BY retry_at, id",
    )?;
    let rows = stmt.query_map([now], |r| r.get(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Schedules (or with `None`, cancels) an automatic retry; counts it as used when scheduled.
pub fn schedule_retry(conn: &Connection, id: &str, at: Option<i64>) -> AppResult<()> {
    conn.execute(
        "UPDATE downloads SET retry_at = ?2, auto_retries = auto_retries + (?2 IS NOT NULL) WHERE id = ?1",
        params![id, at],
    )?;
    Ok(())
}

/// Forgets automatic-retry state (after a manual resume or a success).
pub fn clear_retry(conn: &Connection, id: &str) -> AppResult<()> {
    conn.execute("UPDATE downloads SET retry_at = NULL, auto_retries = 0 WHERE id = ?1", [id])?;
    Ok(())
}

pub fn set_priority(conn: &Connection, id: &str, priority: i32) -> AppResult<()> {
    touch(conn, "UPDATE downloads SET priority = ?2, updated_at = ?3 WHERE id = ?1", id, priority.clamp(-1, 1))
}

pub fn set_queue(conn: &Connection, id: &str, queue_id: &str) -> AppResult<()> {
    touch(conn, "UPDATE downloads SET queue_id = ?2, updated_at = ?3 WHERE id = ?1", id, queue_id)
}

/// Per-download speed limit in bytes/s; `None` = no limit of its own.
pub fn set_speed_limit(conn: &Connection, id: &str, limit: Option<i64>) -> AppResult<()> {
    touch(conn, "UPDATE downloads SET speed_limit = ?2, updated_at = ?3 WHERE id = ?1", id, limit.filter(|n| *n > 0))
}

/// Moves a download to the front (`true`) or back of the queue order.
pub fn move_to_edge(conn: &Connection, id: &str, front: bool) -> AppResult<()> {
    let sql = if front {
        "UPDATE downloads SET position = (SELECT MIN(position) - 1 FROM downloads), updated_at = ?2 WHERE id = ?1"
    } else {
        "UPDATE downloads SET position = (SELECT MAX(position) + 1 FROM downloads), updated_at = ?2 WHERE id = ?1"
    };
    if conn.execute(sql, params![id, now_ms()])? == 0 {
        return Err(AppError::NotFound("That download no longer exists.".into()));
    }
    Ok(())
}

fn touch(conn: &Connection, sql: &str, id: &str, value: impl rusqlite::ToSql) -> AppResult<()> {
    if conn.execute(sql, params![id, value, now_ms()])? == 0 {
        return Err(AppError::NotFound("That download no longer exists.".into()));
    }
    Ok(())
}

/// The most recent download of the same URL, if any (for duplicate detection).
pub fn find_by_url(conn: &Connection, url: &str) -> AppResult<Option<Download>> {
    let sql = format!("SELECT {COLUMNS} FROM downloads WHERE url = ?1 ORDER BY created_at DESC, position DESC LIMIT 1");
    Ok(conn.query_row(&sql, [url], from_row).optional()?)
}

/// Removes finished records (completed and cancelled); files on disk are kept.
pub fn clear_finished(conn: &Connection) -> AppResult<Vec<String>> {
    let mut stmt = conn.prepare("DELETE FROM downloads WHERE status IN ('completed', 'cancelled') RETURNING id")?;
    let rows = stmt.query_map([], |r| r.get(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// How many downloads are still waiting, running or due an automatic retry.
pub fn unfinished_count(conn: &Connection) -> AppResult<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM downloads WHERE status IN ('queued', 'downloading')
            OR (status = 'failed' AND retry_at IS NOT NULL)",
        [],
        |r| r.get(0),
    )?)
}

pub fn delete(conn: &Connection, id: &str) -> AppResult<()> {
    let affected = conn.execute("DELETE FROM downloads WHERE id = ?1", [id])?;
    if affected == 0 {
        return Err(AppError::NotFound("That download no longer exists.".into()));
    }
    Ok(())
}

/// Sets the status and error message (cleared when `None`).
pub fn set_status(
    conn: &Connection,
    id: &str,
    status: DownloadStatus,
    error: Option<&str>,
) -> AppResult<()> {
    conn.execute(
        "UPDATE downloads SET status = ?2, error_message = ?3, updated_at = ?4 WHERE id = ?1",
        params![id, status.as_str(), error, now_ms()],
    )?;
    Ok(())
}

pub fn set_remote_info(conn: &Connection, id: &str, info: &RemoteInfo<'_>) -> AppResult<()> {
    conn.execute(
        "UPDATE downloads SET final_url = ?2, total_bytes = ?3, resumable = ?4, etag = ?5,
                              last_modified = ?6, updated_at = ?7
         WHERE id = ?1",
        params![
            id,
            info.final_url,
            info.total_bytes.map(|n| n as i64),
            info.resumable,
            info.etag,
            info.last_modified,
            now_ms()
        ],
    )?;
    Ok(())
}

/// Points a download at a new link to the same file, keeping its progress.
/// A failed download becomes paused, ready to resume.
pub fn replace_link(conn: &Connection, id: &str, url: &str, info: &RemoteInfo<'_>) -> AppResult<()> {
    conn.execute(
        "UPDATE downloads SET url = ?2, final_url = ?3, total_bytes = ?4, resumable = ?5, etag = ?6,
                              last_modified = ?7, error_message = NULL, retry_at = NULL, auto_retries = 0,
                              status = CASE status WHEN 'failed' THEN 'paused' ELSE status END, updated_at = ?8
         WHERE id = ?1",
        params![
            id,
            url,
            info.final_url,
            info.total_bytes.map(|n| n as i64),
            info.resumable,
            info.etag,
            info.last_modified,
            now_ms()
        ],
    )?;
    Ok(())
}

pub fn set_file_name(conn: &Connection, id: &str, file_name: &str) -> AppResult<()> {
    conn.execute(
        "UPDATE downloads SET file_name = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, file_name, now_ms()],
    )?;
    Ok(())
}

/// Records byte counts for a media download (no segments).
pub fn set_media_progress(conn: &Connection, id: &str, downloaded: u64, total: Option<u64>) -> AppResult<()> {
    conn.execute(
        "UPDATE downloads SET downloaded_bytes = ?2, total_bytes = COALESCE(?3, total_bytes), updated_at = ?4
         WHERE id = ?1",
        params![id, downloaded as i64, total.map(|t| t as i64), now_ms()],
    )?;
    Ok(())
}

pub fn set_temp_path(conn: &Connection, id: &str, temp_path: &str) -> AppResult<()> {
    conn.execute(
        "UPDATE downloads SET temp_path = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, temp_path, now_ms()],
    )?;
    Ok(())
}

/// Clears all progress so the next attempt starts from byte zero.
pub fn reset_progress(conn: &Connection, id: &str) -> AppResult<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM download_segments WHERE download_id = ?1", [id])?;
    tx.execute(
        "UPDATE downloads SET downloaded_bytes = 0, temp_path = NULL, etag = NULL,
                              last_modified = NULL, total_bytes = NULL, updated_at = ?2
         WHERE id = ?1",
        params![id, now_ms()],
    )?;
    tx.commit()?;
    Ok(())
}

/// Turns a finished, failed or cancelled record back into a fresh pending
/// one. The old file (if any) stays; the new one gets a " (1)" name.
pub fn prepare_redownload(conn: &Connection, id: &str) -> AppResult<()> {
    reset_progress(conn, id)?;
    conn.execute(
        "UPDATE downloads SET status = 'pending', saved_path = NULL, completed_at = NULL, error_message = NULL,
                              final_url = NULL, resumable = 0, retry_at = NULL, auto_retries = 0, updated_at = ?2
         WHERE id = ?1",
        params![id, now_ms()],
    )?;
    Ok(())
}

pub fn segments(conn: &Connection, id: &str) -> AppResult<Vec<Segment>> {
    let mut stmt = conn.prepare(
        "SELECT idx, start_byte, end_byte, downloaded FROM download_segments
         WHERE download_id = ?1 ORDER BY idx",
    )?;
    let rows = stmt.query_map([id], |r| {
        Ok(Segment {
            idx: r.get(0)?,
            start: r.get::<_, i64>(1)? as u64,
            end: r.get::<_, i64>(2)? as u64,
            downloaded: r.get::<_, i64>(3)? as u64,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn replace_segments(conn: &Connection, id: &str, segments: &[Segment]) -> AppResult<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM download_segments WHERE download_id = ?1", [id])?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO download_segments (download_id, idx, start_byte, end_byte, downloaded)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for s in segments {
            stmt.execute(params![id, s.idx, s.start as i64, s.end as i64, s.downloaded as i64])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Persists progress: per-segment byte counts (if any) and the overall total.
pub fn save_progress(
    conn: &Connection,
    id: &str,
    segment_progress: &[(u32, u64)],
    downloaded: u64,
) -> AppResult<()> {
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare_cached(
            "UPDATE download_segments SET downloaded = ?3 WHERE download_id = ?1 AND idx = ?2",
        )?;
        for (idx, done) in segment_progress {
            stmt.execute(params![id, idx, *done as i64])?;
        }
    }
    tx.execute(
        "UPDATE downloads SET downloaded_bytes = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, downloaded as i64, now_ms()],
    )?;
    tx.commit()?;
    Ok(())
}

pub fn mark_completed(conn: &Connection, id: &str, saved_path: &str, size: u64) -> AppResult<()> {
    let now = now_ms();
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM download_segments WHERE download_id = ?1", [id])?;
    tx.execute(
        "UPDATE downloads SET status = 'completed', saved_path = ?2, temp_path = NULL,
                              total_bytes = ?3, downloaded_bytes = ?3, error_message = NULL,
                              completed_at = ?4, updated_at = ?4, retry_at = NULL, auto_retries = 0
         WHERE id = ?1",
        params![id, saved_path, size as i64, now],
    )?;
    tx.commit()?;
    Ok(())
}

/// Downloads that were running when the app last exited become paused so
/// the user can resume them from their saved progress.
pub fn recover_interrupted(conn: &Connection) -> AppResult<usize> {
    Ok(conn.execute(
        "UPDATE downloads SET status = 'paused',
                              error_message = 'Interrupted when the app closed. Resume to continue.',
                              updated_at = ?1
         WHERE status = 'downloading'",
        [now_ms()],
    )?)
}

fn from_row(row: &Row<'_>) -> rusqlite::Result<Download> {
    let status: String = row.get(4)?;
    let status = DownloadStatus::parse(&status).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            4,
            rusqlite::types::Type::Text,
            format!("unknown download status {status:?}").into(),
        )
    })?;
    Ok(Download {
        id: row.get(0)?,
        url: row.get(1)?,
        file_name: row.get(2)?,
        destination_dir: row.get(3)?,
        status,
        total_bytes: row.get(5)?,
        downloaded_bytes: row.get(6)?,
        error_message: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        completed_at: row.get(10)?,
        final_url: row.get(11)?,
        saved_path: row.get(12)?,
        resumable: row.get(13)?,
        expected_sha256: row.get(14)?,
        temp_path: row.get(15)?,
        etag: row.get(16)?,
        last_modified: row.get(17)?,
        kind: match row.get::<_, String>(18)?.as_str() {
            "media" => DownloadKind::Media,
            _ => DownloadKind::File,
        },
        title: row.get(19)?,
        media_options: row
            .get::<_, Option<String>>(20)?
            .and_then(|json| serde_json::from_str(&json).ok()),
        referrer: row.get(21)?,
        queue_id: row.get(22)?,
        priority: row.get(23)?,
        position: row.get(24)?,
        category: row.get(25)?,
        speed_limit: row.get(26)?,
        auto_retries: row.get(27)?,
        retry_at: row.get(28)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    fn sample() -> InsertDownload<'static> {
        InsertDownload {
            url: "https://example.com/a.zip",
            file_name: "a.zip",
            destination_dir: "C:\\Downloads",
            expected_sha256: None,
            kind: DownloadKind::File,
            title: None,
            media_options: None,
            referrer: None,
            queue_id: "main",
            priority: 0,
            category: None,
            speed_limit: None,
        }
    }

    #[test]
    fn insert_creates_pending_record() {
        let conn = open_in_memory().unwrap();
        let d = insert(&conn, &sample()).unwrap();
        assert_eq!(d.status, DownloadStatus::Pending);
        assert_eq!(d.downloaded_bytes, 0);
        assert_eq!(d.total_bytes, None);
        assert!(!d.resumable);
        assert_eq!(get(&conn, &d.id).unwrap().unwrap().url, d.url);
    }

    #[test]
    fn list_returns_all_records() {
        let conn = open_in_memory().unwrap();
        insert(&conn, &sample()).unwrap();
        insert(&conn, &sample()).unwrap();
        assert_eq!(list(&conn).unwrap().len(), 2);
    }

    #[test]
    fn delete_missing_is_not_found() {
        let conn = open_in_memory().unwrap();
        let d = insert(&conn, &sample()).unwrap();
        delete(&conn, &d.id).unwrap();
        assert!(matches!(delete(&conn, &d.id), Err(AppError::NotFound(_))));
    }

    #[test]
    fn segments_round_trip_and_progress() {
        let conn = open_in_memory().unwrap();
        let d = insert(&conn, &sample()).unwrap();
        let segs = vec![
            Segment { idx: 0, start: 0, end: 99, downloaded: 0 },
            Segment { idx: 1, start: 100, end: 199, downloaded: 0 },
        ];
        replace_segments(&conn, &d.id, &segs).unwrap();
        save_progress(&conn, &d.id, &[(0, 50), (1, 100)], 150).unwrap();
        let loaded = segments(&conn, &d.id).unwrap();
        assert_eq!(loaded[0].downloaded, 50);
        assert_eq!(loaded[1].remaining(), 0);
        assert_eq!(require(&conn, &d.id).unwrap().downloaded_bytes, 150);
    }

    #[test]
    fn deleting_download_cascades_segments() {
        let conn = open_in_memory().unwrap();
        let d = insert(&conn, &sample()).unwrap();
        replace_segments(&conn, &d.id, &[Segment { idx: 0, start: 0, end: 9, downloaded: 0 }]).unwrap();
        delete(&conn, &d.id).unwrap();
        assert!(segments(&conn, &d.id).unwrap().is_empty());
    }

    #[test]
    fn media_fields_round_trip() {
        use crate::models::{Container, MediaOptions};
        let conn = open_in_memory().unwrap();
        let opts = MediaOptions::Video { max_height: Some(1080), container: Container::Mp4 };
        let d = insert(
            &conn,
            &InsertDownload {
                kind: DownloadKind::Media,
                title: Some("Big Buck Bunny"),
                media_options: Some(&opts),
                ..sample()
            },
        )
        .unwrap();
        assert_eq!(d.kind, DownloadKind::Media);
        assert_eq!(d.title.as_deref(), Some("Big Buck Bunny"));
        assert_eq!(d.media_options, Some(opts));
    }

    #[test]
    fn queue_order_is_priority_then_position() {
        let conn = open_in_memory().unwrap();
        let a = insert(&conn, &sample()).unwrap();
        let b = insert(&conn, &InsertDownload { priority: 1, ..sample() }).unwrap();
        let c = insert(&conn, &InsertDownload { priority: -1, ..sample() }).unwrap();
        let d = insert(&conn, &sample()).unwrap();
        assert!(d.position > a.position);
        for x in [&a, &b, &c, &d] {
            set_status(&conn, &x.id, DownloadStatus::Queued, None).unwrap();
        }
        let order = |conn: &Connection| queued_in_order(conn).unwrap().into_iter().map(|(id, _)| id).collect::<Vec<_>>();
        assert_eq!(order(&conn), [b.id.clone(), a.id.clone(), d.id.clone(), c.id.clone()]);
        move_to_edge(&conn, &d.id, true).unwrap();
        assert_eq!(order(&conn), [b.id.clone(), d.id.clone(), a.id.clone(), c.id.clone()]);
        set_priority(&conn, &c.id, 5).unwrap();
        assert_eq!(require(&conn, &c.id).unwrap().priority, 1);
    }

    #[test]
    fn retries_duplicates_and_clearing() {
        let conn = open_in_memory().unwrap();
        let a = insert(&conn, &sample()).unwrap();
        set_status(&conn, &a.id, DownloadStatus::Failed, Some("x")).unwrap();
        schedule_retry(&conn, &a.id, Some(1000)).unwrap();
        assert_eq!(due_retries(&conn, 999).unwrap().len(), 0);
        assert_eq!(due_retries(&conn, 1000).unwrap(), std::slice::from_ref(&a.id));
        assert_eq!(require(&conn, &a.id).unwrap().auto_retries, 1);
        assert_eq!(unfinished_count(&conn).unwrap(), 1);
        clear_retry(&conn, &a.id).unwrap();
        assert_eq!(require(&conn, &a.id).unwrap().auto_retries, 0);

        assert_eq!(find_by_url(&conn, "https://example.com/a.zip").unwrap().unwrap().id, a.id);
        assert!(find_by_url(&conn, "https://example.com/b.zip").unwrap().is_none());
        mark_completed(&conn, &a.id, "C:\\Downloads\\a.zip", 1).unwrap();
        assert_eq!(clear_finished(&conn).unwrap(), std::slice::from_ref(&a.id));
        assert!(get(&conn, &a.id).unwrap().is_none());
    }

    #[test]
    fn recover_interrupted_pauses_running_downloads() {
        let conn = open_in_memory().unwrap();
        let a = insert(&conn, &sample()).unwrap();
        let b = insert(&conn, &sample()).unwrap();
        set_status(&conn, &a.id, DownloadStatus::Downloading, None).unwrap();
        assert_eq!(recover_interrupted(&conn).unwrap(), 1);
        assert_eq!(require(&conn, &a.id).unwrap().status, DownloadStatus::Paused);
        assert_eq!(require(&conn, &b.id).unwrap().status, DownloadStatus::Pending);
    }
}
