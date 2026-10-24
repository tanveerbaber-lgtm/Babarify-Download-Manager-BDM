//! File categories and the folders they're saved to.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, Row, params};

use crate::error::{AppError, AppResult};
use crate::models::Category;
use crate::validation;

fn from_row(row: &Row<'_>) -> rusqlite::Result<Category> {
    let exts: String = row.get(2)?;
    Ok(Category {
        id: row.get(0)?,
        name: row.get(1)?,
        extensions: exts.split(',').map(str::trim).filter(|e| !e.is_empty()).map(str::to_owned).collect(),
        folder: row.get(3)?,
        sort_order: row.get(4)?,
    })
}

pub fn list(conn: &Connection) -> AppResult<Vec<Category>> {
    let mut stmt = conn.prepare("SELECT id, name, extensions, folder, sort_order FROM categories ORDER BY sort_order, name")?;
    let rows = stmt.query_map([], from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// The category a file name belongs to, by extension.
pub fn for_file_name(conn: &Connection, file_name: &str) -> AppResult<Option<Category>> {
    let Some((_, ext)) = file_name.rsplit_once('.') else { return Ok(None) };
    let ext = ext.to_ascii_lowercase();
    Ok(list(conn)?.into_iter().find(|c| c.extensions.contains(&ext)))
}

/// Gives uncategorized downloads a category from their file name (e.g. after
/// upgrading from a version without categories). Returns how many changed.
pub fn backfill(conn: &Connection) -> AppResult<usize> {
    let rows: Vec<(String, String)> = conn
        .prepare("SELECT id, file_name FROM downloads WHERE category IS NULL")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let mut changed = 0;
    for (id, name) in rows {
        if let Some(c) = for_file_name(conn, &name)? {
            changed += conn.execute("UPDATE downloads SET category = ?2 WHERE id = ?1", params![id, c.id])?;
        }
    }
    Ok(changed)
}

/// Where a category's files go.
pub fn folder_for(category: &Category, default_dir: &Path) -> PathBuf {
    match &category.folder {
        Some(f) => PathBuf::from(f),
        None => default_dir.join(validation::sanitize_file_name(&category.name)),
    }
}

fn normalize_extensions(raw: &[String]) -> AppResult<String> {
    let mut exts: Vec<String> = raw
        .iter()
        .flat_map(|e| e.split([',', ' ', ';']))
        .map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase())
        .filter(|e| !e.is_empty())
        .collect();
    exts.sort();
    exts.dedup();
    if exts.is_empty() || exts.iter().any(|e| e.len() > 10 || !e.chars().all(|c| c.is_ascii_alphanumeric())) {
        return Err(AppError::Validation("List file types like mp4, mkv (letters and digits only).".into()));
    }
    Ok(exts.join(","))
}

/// Creates (no `id`) or updates a category.
pub fn save(conn: &Connection, id: Option<&str>, name: &str, extensions: &[String], folder: Option<&str>) -> AppResult<Category> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 40 {
        return Err(AppError::Validation("Category names are 1–40 characters.".into()));
    }
    let exts = normalize_extensions(extensions)?;
    let folder = match folder.map(str::trim).filter(|f| !f.is_empty()) {
        Some(f) => {
            let p = Path::new(f);
            if !p.is_absolute() {
                return Err(AppError::Validation("The category folder must be a full path.".into()));
            }
            Some(f.to_string())
        }
        None => None,
    };
    let id = match id {
        Some(id) => {
            if conn.execute(
                "UPDATE categories SET name = ?2, extensions = ?3, folder = ?4 WHERE id = ?1",
                params![id, name, exts, folder],
            )? == 0
            {
                return Err(AppError::NotFound("That category no longer exists.".into()));
            }
            id.to_string()
        }
        None => {
            let id = uuid::Uuid::new_v4().to_string();
            let order: i64 = conn.query_row("SELECT COALESCE(MAX(sort_order), 0) + 1 FROM categories", [], |r| r.get(0))?;
            conn.execute(
                "INSERT INTO categories (id, name, extensions, folder, sort_order) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, name, exts, folder, order],
            )?;
            id
        }
    };
    Ok(list(conn)?.into_iter().find(|c| c.id == id).expect("just saved"))
}

/// Deletes a category; its downloads become "other".
pub fn delete(conn: &Connection, id: &str) -> AppResult<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("UPDATE downloads SET category = NULL WHERE category = ?1", [id])?;
    if tx.execute("DELETE FROM categories WHERE id = ?1", [id])? == 0 {
        return Err(AppError::NotFound("That category no longer exists.".into()));
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    #[test]
    fn default_categories_match_by_extension() {
        let conn = open_in_memory().unwrap();
        assert_eq!(for_file_name(&conn, "Movie.MKV").unwrap().unwrap().id, "videos");
        assert_eq!(for_file_name(&conn, "song.mp3").unwrap().unwrap().id, "music");
        assert_eq!(for_file_name(&conn, "setup.exe").unwrap().unwrap().id, "programs");
        assert_eq!(for_file_name(&conn, "a.tar.gz").unwrap().unwrap().id, "compressed");
        assert!(for_file_name(&conn, "README").unwrap().is_none());
        assert!(for_file_name(&conn, "data.bin").unwrap().is_none());
    }

    #[test]
    fn backfills_old_downloads() {
        let conn = open_in_memory().unwrap();
        for name in ["clip.mp4", "notes.bin"] {
            conn.execute(
                "INSERT INTO downloads (id, url, file_name, destination_dir, status, created_at, updated_at) VALUES (?1, 'https://e.com', ?1, 'C:\\', 'completed', 0, 0)",
                [name],
            )
            .unwrap();
        }
        assert_eq!(backfill(&conn).unwrap(), 1);
        let cat: Option<String> = conn.query_row("SELECT category FROM downloads WHERE id = 'clip.mp4'", [], |r| r.get(0)).unwrap();
        assert_eq!(cat.as_deref(), Some("videos"));
    }

    #[test]
    fn folder_defaults_to_subfolder_of_download_dir() {
        let conn = open_in_memory().unwrap();
        let videos = for_file_name(&conn, "x.mp4").unwrap().unwrap();
        assert_eq!(folder_for(&videos, Path::new("C:\\DL")), PathBuf::from("C:\\DL\\Videos"));
    }

    #[test]
    fn saves_and_validates_categories() {
        let conn = open_in_memory().unwrap();
        let c = save(&conn, None, "E-books", &[".EPUB, mobi azw3".into()], None).unwrap();
        assert_eq!(c.extensions, ["azw3", "epub", "mobi"]);
        assert!(save(&conn, None, "Bad", &["../x".into()], None).is_err());
        assert!(save(&conn, None, "Rel", &["abc".into()], Some("relative\\dir")).is_err());
        delete(&conn, &c.id).unwrap();
        assert!(delete(&conn, &c.id).is_err());
    }
}
