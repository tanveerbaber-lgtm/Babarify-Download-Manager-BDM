//! Babarify Download Manager — Rust core.

mod batch;
mod cli;
pub mod browser;
mod commands;
mod completion;
mod cookies;
mod desktop;
mod db;
mod engine;
mod error;
mod media;
mod models;
mod net_guard;
mod request_headers;
mod scanner;
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
use crate::error::AppError;
use crate::media::tools::{self, ToolManager};
use crate::models::Download;
use crate::state::AppState;

const DATABASE_FILE: &str = "bdm.sqlite3";

/// Forwards engine events to the webview.
struct TauriSink(AppHandle);

impl EventSink for TauriSink {
    fn progress(&self, progress: &Progress) {
        let _ = self.0.emit(EVENT_PROGRESS, progress);
        self.0.state::<desktop::Speeds>().record(&progress.id, progress.speed);
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

    fn threat(&self, download: &Download) {
        completion::on_threat(&self.0, download);
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
    pub(crate) url: String,
    pub(crate) page_title: Option<String>,
}

use desktop::show_main_window;

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
async fn handle_browser_request(app: AppHandle, mut request: BrowserRequest) -> BrowserResponse {
    let state = app.state::<AppState>();
    // Sign-in cookies (validated by the pipe server) stay in memory for later
    // requests to the same site, including ones started from BDM's window.
    let had_cookies = match request.take_cookies() {
        Some((url, cookies)) => {
            let any = !cookies.is_empty();
            state.engine.cookies().remember(&url, cookies);
            any
        }
        None => false,
    };
    if let Some((url, headers)) = request.take_headers() {
        state.engine.request_headers().remember(&url, headers);
    }
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
        BrowserRequest::Media { url, page_title, .. } => {
            // Media needs a quality choice, so it opens the dialog rather than starting.
            let _ = app.emit(EVENT_BROWSER_MEDIA, BrowserMedia { url, page_title });
            show_main_window(&app);
            BrowserResponse::ok()
        }
        BrowserRequest::Formats { url, referrer, .. } => {
            let tools = match state.tools.require() {
                Ok(t) => t,
                Err(e) => return BrowserResponse::error(e.to_string()),
            };
            if let Ok(parsed) = url::Url::parse(&url)
                && let Err(b) = state.engine.net_guard().check_url(&parsed).await
            {
                return BrowserResponse::error(b.to_string());
            }
            let cookies = match state.engine.cookies().file_for(&url) {
                Ok(c) => c,
                Err(e) => return BrowserResponse::error(e.to_string()),
            };
            let headers = state.engine.request_headers().for_url(&url);
            match media::analyze::analyze(&tools, &url, referrer.as_deref(), &headers, cookies.as_ref().map(|c| c.path())).await {
                Ok(info) => match media::choices::media_choices(&info) {
                    Ok(choices) => BrowserResponse::with_media(choices),
                    Err(message) => BrowserResponse::error(message),
                },
                Err(AppError::Validation(message)) => BrowserResponse::media_error(message, had_cookies),
                Err(e) => BrowserResponse::error(e.to_string()),
            }
        }
        BrowserRequest::MediaDownload { url, title, options, label, size, referrer, .. } if confirm_browser_downloads(&state) => {
            match commands::propose_media(&state, &url, title.as_deref(), &options, label.as_deref(), size, referrer.as_deref()) {
                Ok(p) => ask_user(&app, p),
                Err(e) => BrowserResponse::error(e.to_string()),
            }
        }
        BrowserRequest::MediaDownload { url, title, options, referrer, .. } => {
            let items = [commands::MediaItem { url, title, referrer }];
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
        .plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
            // A second launch hands over its links and options (the first item is the program).
            let rest = args.get(1..).unwrap_or_default();
            if rest.is_empty() {
                show_main_window(app);
            } else if !rest.iter().all(|a| a == desktop::AUTOSTART_ARG) {
                cli::run(app, rest, std::path::Path::new(&cwd));
            }
            // A second "Start with Windows" launch leaves the running BDM in the tray.
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(desktop::Speeds::default())
        .manage(desktop::PendingOffer::default())
        .manage(cli::StartupRequests::default())
        .on_window_event(|window, event| {
            if window.label() == "main"
                && let tauri::WindowEvent::CloseRequested { api, .. } = event
                && desktop::close_to_tray(window.app_handle())
            {
                api.prevent_close();
            }
        })
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
                // The window starts hidden; "Start with Windows" launches stay in the tray.
                if !std::env::args().any(|a| a == desktop::AUTOSTART_ARG) {
                    let _ = main.show();
                }
            }
            if let Err(e) = desktop::create_tray(app.handle()) {
                eprintln!("[bdm] couldn't create the tray icon: {e}");
            }
            let start_with_windows = db::settings::load(&*database.lock()?, &fallback_download_dir)?.start_with_windows;
            // Re-written every launch so a moved or upgraded install keeps starting.
            if start_with_windows && let Err(e) = desktop::set_start_with_windows(true) {
                eprintln!("[bdm] couldn't update Windows startup: {e}");
            }

            app.manage(AppState { database, engine, tools, data_dir: data_dir.clone(), fallback_download_dir });
            desktop::watch_clipboard(app.handle().clone());
            #[cfg(all(windows, not(debug_assertions)))]
            if let Err(e) = desktop::register_url_protocol() {
                eprintln!("[bdm] couldn't register bdm:// links: {e}");
            }
            // Links and options BDM was started with wait until the window has loaded.
            let args: Vec<String> = std::env::args().skip(1).collect();
            if !args.is_empty() {
                let cwd = std::env::current_dir().unwrap_or_default();
                cli::run(app.handle(), &args, &cwd);
            }

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
            commands::refresh_download_link,
            commands::disk_free_space,
            desktop::get_clipboard_offer,
            desktop::accept_clipboard_offer,
            desktop::dismiss_clipboard_offer,
            cli::take_startup_requests,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Babarify Download Manager");
}
