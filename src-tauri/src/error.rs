//! Application error type shared by every Tauri command.
//!
//! Errors are serialized as `{ "kind": "...", "message": "..." }` so the
//! frontend can branch on `kind` without parsing strings.

use serde::Serialize;

#[derive(Debug, thiserror::Error, Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "camelCase")]
pub enum AppError {
    /// User-supplied input was rejected (bad URL, unsafe filename, ...).
    #[error("{0}")]
    Validation(String),

    /// The requested record does not exist.
    #[error("{0}")]
    NotFound(String),

    /// A database operation failed.
    #[error("database error: {0}")]
    Database(String),

    /// A filesystem operation failed.
    #[error("filesystem error: {0}")]
    Io(String),

    /// An unexpected internal failure (poisoned lock, path resolution, ...).
    #[error("internal error: {0}")]
    Internal(String),
}

pub type AppResult<T> = Result<T, AppError>;

impl From<rusqlite::Error> for AppError {
    fn from(err: rusqlite::Error) -> Self {
        AppError::Database(err.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::Io(err.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        AppError::Internal(err.to_string())
    }
}
