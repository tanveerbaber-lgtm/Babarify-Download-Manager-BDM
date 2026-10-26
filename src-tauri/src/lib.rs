//! Babarify Download Manager — Rust core.

mod batch;
pub mod browser;
mod commands;
mod completion;
mod db;
mod engine;
mod error;
mod media;
mod models;
mod net_guard;
mod state;
mod title_bar;
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

    fn completed(&self, download: &Download) {
        completion::on_completed(&self.0, download);
    }

    fn failed(&self, download: &Download) {
        completion::on_failed(&self.0, download);
    }

    fn all_done(&self) {
        completion::on_all_done(&self.0);
    }

    fn queues_changed(&self) {
        let _ = self.0.emit(EVENT_QUEUES_CHANGED, ());
    }
}

pub const EVENT_QUEUES_CHANGED: &str = "queues:changed";

pub const EVENT_BROWSER_ADDED: &str = "browser:added";
/// A browser request waiting for the "New Download" dialog.
pub const EVENT_BROWSER_CONFIRM: &str = "browser:confirm";
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

fn confirm_browser_downloads(state: &AppState) -> bool {
    state
        .db()
        .and_then(|c| db::settings::load(&c, &state.fallback_download_dir))
        .map(|s| s.confirm_browser_downloads)
        .unwrap_or(true)
}

/// Shows the confirm dialog for a browser request instead of starting it.
fn ask_user(app: &AppHandle, proposal: commands::BrowserProposal) -> BrowserResponse {
    let _ = app.emit(EVENT_BROWSER_CONFIRM, &proposal);
    show_main_window(app);
    BrowserResponse::asked()
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
        BrowserRequest::Download { url, file_name, referrer, size, .. } if confirm_browser_downloads(&state) => {
            match commands::propose_file(&state, &url, file_name.as_deref(), referrer.as_deref(), size) {
                Ok(p) => ask_user(&app, p),
                Err(e) => BrowserResponse::error(e.to_string()),
            }
        }
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
            if let Ok(parsed) = url::Url::parse(&url)
                && let Err(b) = state.engine.net_guard().check_url(&parsed).await
            {
                return BrowserResponse::error(b.to_string());
            }
            match media::analyze::analyze(&tools, &url).await {
                Ok(info) => match media::choices::media_choices(&info) {
                    Ok(choices) => BrowserResponse::with_media(choices),
                    Err(message) => BrowserResponse::error(message),
                },
                Err(e) => BrowserResponse::error(e.to_string()),
            }
        }
        BrowserRequest::MediaDownload { url, title, options, label, size } if confirm_browser_downloads(&state) => {
            match commands::propose_media(&state, &url, title.as_deref(), &options, label.as_deref(), size) {
                Ok(p) => ask_user(&app, p),
                Err(e) => BrowserResponse::error(e.to_string()),
            }
        }
        BrowserRequest::MediaDownload { url, title, options, .. } => {
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
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let paths = app.path();
            let data_dir = paths.app_data_dir()?;
            let fallback_download_dir = paths
                .download_dir()
                .or_else(|_| paths.home_dir())?
                .to_string_lossy()
                .into_owned();

            let database = Arc::new(db::Database::new(db::open(&data_dir.join(DATABASE_FILE))?));
            if let Err(e) = db::categories::backfill(&*database.lock()?) {
                eprintln!("[bdm] couldn't sort old downloads into categories: {e}");
            }
            let runtime = tauri::async_runtime::block_on(async { tokio::runtime::Handle::current() });
            let sink: Arc<dyn EventSink> = Arc::new(TauriSink(app.handle().clone()));
            let config = EngineConfig::default();

            let tools_dir = data_dir.join("tools");
            tools::clean_old_copies(&tools_dir);
            // Tools only ever come from GitHub, so their client never needs the local network.
            let tool_client = engine::http::build_client(&config, net_guard::NetGuard::new(false))?;
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

            // Installed (release) builds point Chrome and Edge at their own helper on
            // every launch, so a fresh install or an upgrade to a new folder just works.
            #[cfg(all(windows, not(debug_assertions)))]
            if let Ok(host) = browser::registry::host_exe_path()
                && host.metadata().is_ok_and(|m| m.len() > 0)
                && browser::registry::needs_registration(&data_dir, &host)
                && let Err(e) = browser::registry::register(&data_dir, &host)
            {
                eprintln!("[bdm] couldn't register the browser helper: {e}");
            }

            if let Some(main) = app.get_webview_window("main") {
                title_bar::paint(&main);
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
            commands::add_batch,
            commands::expand_batch_pattern,
            commands::import_url_list,
            commands::find_duplicate,
            commands::list_queues,
            commands::save_queue,
            commands::delete_queue,
            commands::start_queue,
            commands::stop_queue,
            commands::list_categories,
            commands::save_category,
            commands::delete_category,
            commands::set_download_priority,
            commands::move_downloads_to_queue,
            commands::move_downloads_in_queue,
            commands::set_download_speed_limit,
            commands::redownload,
            commands::clear_finished,
            commands::open_download,
            commands::show_in_folder,
            commands::file_sha256,
            commands::bulk_action,
            commands::open_download_window,
            commands::open_legal_document,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Babarify Download Manager");
}
