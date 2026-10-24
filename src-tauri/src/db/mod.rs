//! SQLite storage: connection setup and versioned schema migrations.
//!
//! The schema version is tracked with `PRAGMA user_version`. Each migration
//! runs inside its own transaction together with the version bump, so a
//! failed migration leaves the database at the previous version.

pub mod categories;
pub mod downloads;
pub mod queues;
pub mod settings;

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

/// Shared handle to the single SQLite connection.
pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    pub fn new(conn: Connection) -> Self {
        Self { conn: Mutex::new(conn) }
    }

    pub fn lock(&self) -> AppResult<MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| AppError::Internal("database lock poisoned".into()))
    }
}

/// Ordered list of migrations. Migration `i` upgrades the schema from
/// version `i` to `i + 1`. Never edit or reorder a released migration —
/// append a new one instead.
const MIGRATIONS: &[&str] = &[
    include_str!("migrations/0001_initial.sql"),
    include_str!("migrations/0002_engine.sql"),
    include_str!("migrations/0003_media.sql"),
    include_str!("migrations/0004_browser.sql"),
    include_str!("migrations/0005_queues.sql"),
];

/// Latest schema version this build understands.
pub const SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;

/// Opens (creating if needed) the database file and brings it up to date.
pub fn open(path: &Path) -> AppResult<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut conn = Connection::open(path)?;
    configure(&conn)?;
    migrate(&mut conn)?;
    Ok(conn)
}

/// Opens a fresh in-memory database with the full schema (tests only).
#[cfg(test)]
pub fn open_in_memory() -> AppResult<Connection> {
    let mut conn = Connection::open_in_memory()?;
    configure(&conn)?;
    migrate(&mut conn)?;
    Ok(conn)
}

fn configure(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;",
    )?;
    Ok(())
}

pub fn schema_version(conn: &Connection) -> AppResult<u32> {
    Ok(conn.query_row("PRAGMA user_version", [], |row| row.get(0))?)
}

fn migrate(conn: &mut Connection) -> AppResult<()> {
    let current = schema_version(conn)?;
    if current > SCHEMA_VERSION {
        return Err(AppError::Database(format!(
            "database schema version {current} is newer than this build supports ({SCHEMA_VERSION}); \
             please update Babarify Download Manager"
        )));
    }
    for (index, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let target = index as u32 + 1;
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", target)?;
        tx.commit()?;
        log_migration(target);
    }
    Ok(())
}

fn log_migration(version: u32) {
    #[cfg(debug_assertions)]
    eprintln!("[bdm] applied database migration v{version}");
    #[cfg(not(debug_assertions))]
    let _ = version;
}

/// Current time as unix epoch milliseconds.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_database_reaches_latest_version() {
        let conn = open_in_memory().unwrap();
        assert_eq!(schema_version(&conn).unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn reopening_file_database_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("bdm.sqlite3");
        drop(open(&path).unwrap());
        let conn = open(&path).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn newer_schema_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bdm.sqlite3");
        {
            let conn = Connection::open(&path).unwrap();
            conn.pragma_update(None, "user_version", SCHEMA_VERSION + 1).unwrap();
        }
        assert!(open(&path).is_err());
    }
}
