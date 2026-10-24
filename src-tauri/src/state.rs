//! Shared application state managed by Tauri.

use std::path::PathBuf;
use std::sync::{Arc, MutexGuard};

use rusqlite::Connection;

use crate::db::Database;
use crate::engine::Engine;
use crate::error::AppResult;
use crate::media::tools::ToolManager;

pub struct AppState {
    pub database: Arc<Database>,
    pub engine: Engine,
    pub tools: Arc<ToolManager>,
    pub data_dir: PathBuf,
    /// The OS "Downloads" folder, used until the user picks a default.
    pub fallback_download_dir: String,
}

impl AppState {
    pub fn db(&self) -> AppResult<MutexGuard<'_, Connection>> {
        self.database.lock()
    }
}
