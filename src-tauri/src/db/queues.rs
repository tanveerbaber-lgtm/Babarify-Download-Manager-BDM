//! Download queues and their schedules.

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::db::now_ms;
use crate::error::{AppError, AppResult};
use crate::models::{Queue, QueueSchedule};

pub const MAIN_QUEUE: &str = "main";
const MAX_NAME: usize = 40;

fn from_row(row: &Row<'_>) -> rusqlite::Result<Queue> {
    let schedule: String = row.get(2)?;
    Ok(Queue {
        id: row.get(0)?,
        name: row.get(1)?,
        schedule: serde_json::from_str(&schedule).unwrap_or_default(),
        running: row.get(3)?,
        sort_order: row.get(4)?,
    })
}

pub fn list(conn: &Connection) -> AppResult<Vec<Queue>> {
    let mut stmt = conn.prepare("SELECT id, name, schedule, running, sort_order FROM queues ORDER BY sort_order, created_at")?;
    let rows = stmt.query_map([], from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn get(conn: &Connection, id: &str) -> AppResult<Option<Queue>> {
    Ok(conn
        .query_row("SELECT id, name, schedule, running, sort_order FROM queues WHERE id = ?1", [id], from_row)
        .optional()?)
}

pub fn require(conn: &Connection, id: &str) -> AppResult<Queue> {
    get(conn, id)?.ok_or_else(|| AppError::NotFound("That queue no longer exists.".into()))
}

/// "HH:MM" → minutes after midnight.
pub fn parse_time(value: &str) -> Option<u32> {
    let (h, m) = value.split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60 && value.len() == 5).then_some(h * 60 + m)
}

pub fn validate_schedule(s: &QueueSchedule) -> AppResult<()> {
    if parse_time(&s.start).is_none() || s.stop.as_deref().is_some_and(|t| parse_time(t).is_none()) {
        return Err(AppError::Validation("Use 24-hour times like 01:30.".into()));
    }
    if s.stop.as_deref() == Some(s.start.as_str()) {
        return Err(AppError::Validation("Start and stop times must differ.".into()));
    }
    if s.days.is_empty() || s.days.iter().any(|d| *d > 6) {
        return Err(AppError::Validation("Choose at least one day.".into()));
    }
    Ok(())
}

/// Creates a queue (empty `id`) or updates an existing one.
pub fn save(conn: &Connection, name: &str, schedule: &QueueSchedule, id: Option<&str>) -> AppResult<Queue> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME {
        return Err(AppError::Validation(format!("Queue names are 1–{MAX_NAME} characters.")));
    }
    validate_schedule(schedule)?;
    let mut days = schedule.days.clone();
    days.sort_unstable();
    days.dedup();
    let schedule = QueueSchedule { days, ..schedule.clone() };
    let json = serde_json::to_string(&schedule)?;
    let id = match id {
        Some(id) => {
            require(conn, id)?;
            conn.execute("UPDATE queues SET name = ?2, schedule = ?3 WHERE id = ?1", params![id, name, json])?;
            id.to_string()
        }
        None => {
            let id = uuid::Uuid::new_v4().to_string();
            let order: i64 = conn.query_row("SELECT COALESCE(MAX(sort_order), 0) + 1 FROM queues", [], |r| r.get(0))?;
            conn.execute(
                // A scheduled queue waits for its window; a manual one is ready at once.
                "INSERT INTO queues (id, name, schedule, running, sort_order, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, name, json, !schedule.enabled, order, now_ms()],
            )?;
            id
        }
    };
    require(conn, &id)
}

/// Deletes a queue; its downloads move to Main.
pub fn delete(conn: &Connection, id: &str) -> AppResult<()> {
    if id == MAIN_QUEUE {
        return Err(AppError::Validation("The Main queue can't be deleted.".into()));
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute("UPDATE downloads SET queue_id = ?2 WHERE queue_id = ?1", params![id, MAIN_QUEUE])?;
    if tx.execute("DELETE FROM queues WHERE id = ?1", [id])? == 0 {
        return Err(AppError::NotFound("That queue no longer exists.".into()));
    }
    tx.commit()?;
    Ok(())
}

pub fn set_running(conn: &Connection, id: &str, running: bool) -> AppResult<()> {
    if conn.execute("UPDATE queues SET running = ?2 WHERE id = ?1", params![id, running])? == 0 {
        return Err(AppError::NotFound("That queue no longer exists.".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    fn schedule(start: &str, stop: Option<&str>) -> QueueSchedule {
        QueueSchedule { enabled: true, start: start.into(), stop: stop.map(Into::into), days: vec![0, 1, 2, 3, 4] }
    }

    #[test]
    fn main_queue_exists_and_cannot_be_deleted() {
        let conn = open_in_memory().unwrap();
        let qs = list(&conn).unwrap();
        assert_eq!(qs[0].id, MAIN_QUEUE);
        assert!(!qs[0].schedule.enabled);
        assert!(delete(&conn, MAIN_QUEUE).is_err());
    }

    #[test]
    fn creates_updates_and_deletes_queues() {
        let conn = open_in_memory().unwrap();
        let q = save(&conn, " Night ", &schedule("01:00", Some("06:00")), None).unwrap();
        assert_eq!(q.name, "Night");
        assert_eq!(q.schedule.stop.as_deref(), Some("06:00"));
        assert!(!q.running, "a scheduled queue waits for its window");
        let q2 = save(&conn, "Overnight", &schedule("23:00", Some("07:00")), Some(&q.id)).unwrap();
        assert_eq!(q2.id, q.id);
        set_running(&conn, &q.id, false).unwrap();
        assert!(!require(&conn, &q.id).unwrap().running);
        delete(&conn, &q.id).unwrap();
        assert_eq!(list(&conn).unwrap().len(), 1);
    }

    #[test]
    fn rejects_bad_schedules() {
        let conn = open_in_memory().unwrap();
        assert!(save(&conn, "x", &schedule("25:00", None), None).is_err());
        assert!(save(&conn, "x", &schedule("1:00", None), None).is_err());
        assert!(save(&conn, "x", &schedule("02:00", Some("02:00")), None).is_err());
        assert!(save(&conn, "x", &QueueSchedule { days: vec![], ..schedule("01:00", None) }, None).is_err());
        assert!(save(&conn, "", &schedule("01:00", None), None).is_err());
        assert_eq!(parse_time("07:45"), Some(465));
    }
}
