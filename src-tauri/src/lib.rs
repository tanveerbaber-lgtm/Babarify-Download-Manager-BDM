//! Babarify Download Manager — Rust core.

pub mod browser;
mod commands;
mod db;
mod engine;
mod error;
mod media;
mod models;
mod state;
mod validation;

#[cfg(test)]
#[path = "../testsupport/server.rs"]
mod test_server;

use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::engine::events::{
    EVENT_PROGRESS, EVENT_REMOVED, EVENT_TOOL_PROGRESS, EVENT_UPDATED, EventSink, Progress, ToolProgress,
};
use crate::browser::ipc;
use crate::browser::protocol::{BrowserRequest, BrowserResponse};
use crate::engine::{Engine, EngineConfig};
use crate::media::tools::{self, ToolManager};
use crate::models::Download;
use crate::state::AppState;

const DATABASE_FILE: &str = "bdm.sqlite3";

/// Forwards engine events to the webview.
struct TauriSink(AppHandle);

impl EventSink for TauriSink {
    fn progress(&self, progress: &Progress) {
        let _ = self.0.emit(EVENT_PROGRESS, progress);
    }

    fn updated(&self, download: &Download) {
        let _ = self.0.emit(EVENT_UPDATED, download);
    }

    fn removed(&self, id: &str) {
        let _ = self.0.emit(EVENT_REMOVED, id);
    }

    fn tool_progress(&self, progress: &ToolProgress) {
        let _ = self.0.emit(EVENT_TOOL_PROGRESS, progress);
    }
}

pub const EVENT_BROWSER_ADDED: &str = "browser:added";
pub const EVENT_BROWSER_MEDIA: &str = "browser:media";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BrowserMedia {
    url: String,
    page_title: Option<String>,
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn show_on_browser_add(state: &AppState) -> bool {
    state
        .db()
        .and_then(|c| db::settings::load(&c, &state.fallback_download_dir))
        .map(|s| s.show_on_browser_add)
        .unwrap_or(true)
}

/// Handles a request from the extension, relayed by the native host.
async fn handle_browser_request(app: AppHandle, request: BrowserRequest) -> BrowserResponse {
    let state = app.state::<AppState>();
    match request {
        BrowserRequest::Ping => BrowserResponse::running(&app.package_info().version.to_string()),
        BrowserRequest::Download { url, file_name, referrer, .. } => {
            match commands::add_from_browser(&state, &url, file_name.as_deref(), referrer.as_deref()) {
                Ok(download) => {
                    let _ = app.emit(EVENT_BROWSER_ADDED, &download);
                    if show_on_browser_add(&state) {
                        show_main_window(&app);
                    }
                    BrowserResponse::ok()
                }
                Err(e) => BrowserResponse::error(e.to_string()),
            }
        }
        BrowserRequest::Media { url, page_title } => {
            // Media needs a quality choice, so it opens the dialog rather than starting.
            let _ = app.emit(EVENT_BROWSER_MEDIA, BrowserMedia { url, page_title });
            show_main_window(&app);
            BrowserResponse::ok()
        }
        BrowserRequest::Formats { url } => {
            let tools = match state.tools.require() {
                Ok(t) => t,
                Err(e) => return BrowserResponse::error(e.to_string()),
            };
            match media::analyze::analyze(&tools, &url).await {
                Ok(info) => match media::choices::media_choices(&info) {
                    Ok(choices) => BrowserResponse::with_media(choices),
                    Err(message) => BrowserResponse::error(message),
                },
                Err(e) => BrowserResponse::error(e.to_string()),
            }
        }
        BrowserRequest::MediaDownload { url, title, options } => {
            let items = [commands::MediaItem { url, title }];
            match commands::add_media(&state, &items, &options, None, true) {
                Ok(created) => {
                    for d in &created {
                        let _ = app.emit(EVENT_BROWSER_ADDED, d);
                    }
                    if show_on_browser_add(&state) {
                        show_main_window(&app);
                    }
                    BrowserResponse::ok()
                }
                Err(e) => BrowserResponse::error(e.to_string()),
            }
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Must be first: a second launch (e.g. by the browser helper) just
        // focuses the running instance.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main_window(app)))
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let paths = app.path();
            let data_dir = paths.app_data_dir()?;
            let fallback_download_dir = paths
                .download_dir()
                .or_else(|_| paths.home_dir())?
                .to_string_lossy()
                .into_owned();

            let database = Arc::new(db::Database::new(db::open(&data_dir.join(DATABASE_FILE))?));
            let runtime = tauri::async_runtime::block_on(async { tokio::runtime::Handle::current() });
            let sink: Arc<dyn EventSink> = Arc::new(TauriSink(app.handle().clone()));
            let config = EngineConfig::default();

            let tools_dir = data_dir.join("tools");
            tools::clean_old_copies(&tools_dir);
            let tool_client = engine::http::build_client(&config)?;
            let tools = Arc::new(ToolManager::new(tools_dir, tool_client, sink.clone()));

            let engine = Engine::new(database.clone(), sink, config, runtime.clone(), tools.clone())?;
            engine.start_up()?;

            let auto_update = db::settings::load(&*database.lock()?, &fallback_download_dir)?.auto_update_yt_dlp;
            if auto_update {
                let tools = tools.clone();
                runtime.spawn(async move {
                    if let Err(e) = tools.auto_update_yt_dlp().await {
                        eprintln!("[bdm] yt-dlp auto-update skipped: {e}");
                    }
                });
            }

            app.manage(AppState { database, engine, tools, data_dir: data_dir.clone(), fallback_download_dir });

            // Local endpoint for the browser helper. A fresh token each launch
            // means only processes that can read this user's AppData get in.
            let token = ipc::write_session_files(&data_dir)?;
            let handle = app.handle().clone();
            runtime.spawn(async move {
                let handler_app = handle.clone();
                let handler: ipc::Handler =
                    Arc::new(move |req| Box::pin(handle_browser_request(handler_app.clone(), req)));
                #[cfg(windows)]
                if let Err(e) = ipc::serve(ipc::pipe_name(), token, handler).await {
                    eprintln!("[bdm] browser integration unavailable: {e}");
                }
                #[cfg(not(windows))]
                let _ = (token, handler);
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_info,
            commands::list_downloads,
            commands::add_download,
            commands::resume_download,
            commands::pause_download,
            commands::cancel_download,
            commands::remove_download,
            commands::suggest_file_name,
            commands::get_settings,
            commands::update_settings,
            commands::get_media_tools,
            commands::install_media_tools,
            commands::check_media_tool_updates,
            commands::analyze_media,
            commands::add_media_downloads,
            commands::get_browser_integration,
            commands::register_browser_integration,
            commands::unregister_browser_integration,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Babarify Download Manager");
}
