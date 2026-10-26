//! Windows desktop integration: the tray icon, closing to the tray, starting
//! with Windows, and offering copied download links.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_notification::NotificationExt;

use crate::browser::protocol::BrowserRequest;
use crate::db::{downloads, settings};
use crate::error::{AppError, AppResult};
use crate::models::{DownloadStatus, Settings};
use crate::state::AppState;
use crate::validation;

/// Passed by the "Start with Windows" entry: start in the tray, not on screen.
pub const AUTOSTART_ARG: &str = "--minimized";
pub const OFFER_WINDOW: &str = "clipboard-offer";
/// Asks the main window to open its Add dialog.
pub const EVENT_ADD_DOWNLOAD: &str = "app:add-download";
/// Tells the offer window a newer link replaced the one it shows.
pub const EVENT_OFFER_CHANGED: &str = "offer:changed";

const TRAY_ID: &str = "bdm-tray";
const APP_NAME: &str = "Babarify Download Manager";
#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const CLIPBOARD_POLL: Duration = Duration::from_millis(700);
const TOOLTIP_EVERY: Duration = Duration::from_secs(2);
/// A download's speed counts towards the total while it reported this recently.
const SPEED_FRESH: Duration = Duration::from_secs(3);
const OFFER_SIZE: (f64, f64) = (360.0, 96.0);
const OFFER_MARGIN: f64 = 12.0;

/// File types a copied link is offered for (the extension's default capture list).
const OFFER_EXTENSIONS: &[&str] = &[
    "zip", "rar", "7z", "tar", "gz", "bz2", "xz", "iso", "img", "exe", "msi", "msix", "appx", "dmg", "pkg", "deb", "rpm",
    "apk", "mp4", "mkv", "webm", "avi", "mov", "m4v", "wmv", "flv", "mp3", "m4a", "flac", "wav", "ogg", "opus", "aac",
    "pdf", "epub", "bin", "torrent",
];

/// Latest speed of each running download, for the tray tooltip.
#[derive(Default)]
pub struct Speeds(Mutex<HashMap<String, (f64, Instant)>>);

impl Speeds {
    pub fn record(&self, id: &str, speed: f64) {
        if let Ok(mut map) = self.0.lock() {
            map.insert(id.to_owned(), (speed, Instant::now()));
        }
    }

    /// Downloads moving right now and their combined speed in bytes/s.
    fn current(&self) -> (usize, f64) {
        let Ok(mut map) = self.0.lock() else { return (0, 0.0) };
        map.retain(|_, (_, at)| at.elapsed() < SPEED_FRESH);
        (map.len(), map.values().map(|(s, _)| s).sum())
    }
}

/// The copied link the offer window is showing.
#[derive(Default)]
pub struct PendingOffer(Mutex<Option<String>>);

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardOffer {
    pub url: String,
    pub file_name: String,
    pub host: String,
}

fn current_settings(app: &AppHandle) -> Option<Settings> {
    let state = app.try_state::<AppState>()?;
    let conn = state.db().ok()?;
    settings::load(&conn, &state.fallback_download_dir).ok()
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

// ------------------------------------------------------------------ tray

/// "2 downloading · 1.3 MB/s"
fn human_speed(bytes_per_sec: f64) -> String {
    let units = ["B", "KB", "MB", "GB"];
    let mut v = bytes_per_sec.max(0.0);
    let mut i = 0;
    while v >= 1024.0 && i < units.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 || v >= 100.0 { format!("{v:.0} {}/s", units[i]) } else { format!("{v:.1} {}/s", units[i]) }
}

fn tooltip_text(active: usize, speed: f64) -> String {
    match active {
        0 => APP_NAME.to_owned(),
        n => format!("{APP_NAME}\n{n} downloading · {}", human_speed(speed)),
    }
}

/// Pauses everything running or queued, or resumes everything paused.
fn act_on_all(app: &AppHandle, pause: bool) {
    let Some(state) = app.try_state::<AppState>() else { return };
    let Ok(list) = state.db().and_then(|c| downloads::list(&c)) else { return };
    for d in list {
        let result = match d.status {
            DownloadStatus::Downloading | DownloadStatus::Queued if pause => state.engine.pause(&d.id),
            DownloadStatus::Paused if !pause => state.engine.enqueue(&d.id),
            _ => Ok(()),
        };
        if let Err(e) = result {
            eprintln!("[bdm] tray action skipped {}: {e}", d.id);
        }
    }
}

pub fn create_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open BDM", true, None::<&str>)?;
    let add = MenuItem::with_id(app, "add", "Add Download…", true, None::<&str>)?;
    let resume = MenuItem::with_id(app, "resume-all", "Resume All", true, None::<&str>)?;
    let pause = MenuItem::with_id(app, "pause-all", "Pause All", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit BDM", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[&open, &add, &PredefinedMenuItem::separator(app)?, &resume, &pause, &PredefinedMenuItem::separator(app)?, &quit],
    )?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(APP_NAME)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main_window(app),
            "add" => {
                show_main_window(app);
                let _ = app.emit_to("main", EVENT_ADD_DOWNLOAD, ());
            }
            "resume-all" => act_on_all(app, false),
            "pause-all" => act_on_all(app, true),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;

    // Keep the tooltip's speed current.
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut last = String::new();
        loop {
            tokio::time::sleep(TOOLTIP_EVERY).await;
            let (active, speed) = handle.state::<Speeds>().current();
            let text = tooltip_text(active, speed);
            if text != last
                && let Some(tray) = handle.tray_by_id(TRAY_ID)
            {
                let _ = tray.set_tooltip(Some(&text));
                last = text;
            }
        }
    });
    Ok(())
}

/// Shown once per session the first time the window is closed to the tray.
static TRAY_HINT_SHOWN: AtomicBool = AtomicBool::new(false);

/// Closing the main window hides it to the tray when that setting is on.
/// Returns whether the close was turned into a hide.
pub fn close_to_tray(app: &AppHandle) -> bool {
    if !current_settings(app).is_some_and(|s| s.close_to_tray) {
        return false;
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    if !TRAY_HINT_SHOWN.swap(true, Ordering::Relaxed) {
        let _ = app
            .notification()
            .builder()
            .title("BDM Is Still Running")
            .body("Downloads continue in the background. Click the BDM icon in the tray to open it, or right-click it to quit.")
            .show();
    }
    true
}

// --------------------------------------------------------- start with Windows

/// The Run entry's command: this exe, started in the tray.
fn run_command(exe: &std::path::Path) -> String {
    format!("\"{}\" {AUTOSTART_ARG}", exe.display())
}

/// Adds or removes BDM's per-user "Run" entry. Re-written on every launch
/// while on, so a moved or upgraded install keeps starting.
pub fn set_start_with_windows(enabled: bool) -> AppResult<()> {
    #[cfg(windows)]
    {
        use winreg::RegKey;
        use winreg::enums::HKEY_CURRENT_USER;
        let (run, _) = RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey(RUN_KEY)
            .map_err(|e| AppError::Io(format!("couldn't open the Windows startup list: {e}")))?;
        if enabled {
            let exe = std::env::current_exe().map_err(|e| AppError::Io(e.to_string()))?;
            run.set_value(APP_NAME, &run_command(&exe))
                .map_err(|e| AppError::Io(format!("couldn't add BDM to Windows startup: {e}")))?;
        } else if let Err(e) = run.delete_value(APP_NAME)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            return Err(AppError::Io(format!("couldn't remove BDM from Windows startup: {e}")));
        }
    }
    #[cfg(not(windows))]
    let _ = enabled;
    Ok(())
}

// ------------------------------------------------------------ bdm:// links

/// The command Windows runs for a `bdm://` link.
#[cfg_attr(debug_assertions, allow(dead_code))]
fn url_protocol_command(exe: &std::path::Path) -> String {
    format!("\"{}\" \"%1\"", exe.display())
}

/// Registers `bdm://` links for this user (HKCU, no admin), pointing at this
/// exe. Re-written every launch like the browser helper; the uninstaller
/// removes it.
#[cfg(windows)]
#[cfg_attr(debug_assertions, allow(dead_code))]
pub fn register_url_protocol() -> AppResult<()> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    let exe = std::env::current_exe().map_err(|e| AppError::Io(e.to_string()))?;
    let io = |e: std::io::Error| AppError::Io(format!("couldn't register bdm:// links: {e}"));
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(r"Software\Classes\bdm").map_err(io)?;
    key.set_value("", &format!("URL:{APP_NAME}")).map_err(io)?;
    key.set_value("URL Protocol", &"").map_err(io)?;
    let (icon, _) = key.create_subkey("DefaultIcon").map_err(io)?;
    icon.set_value("", &format!("\"{}\",0", exe.display())).map_err(io)?;
    let (command, _) = key.create_subkey(r"shell\open\command").map_err(io)?;
    command.set_value("", &url_protocol_command(&exe)).map_err(io)?;
    Ok(())
}

// ------------------------------------------------------------ clipboard

/// A copied text that is worth offering: one http(s) link to a file type BDM
/// handles, with no credentials in it.
pub fn offerable_link(text: &str) -> Option<url::Url> {
    let text = text.trim();
    if text.is_empty() || text.len() > 2048 || text.chars().any(char::is_whitespace) {
        return None;
    }
    let url = validation::validate_url(text).ok()?;
    let name = url.path_segments()?.next_back()?.to_ascii_lowercase();
    let ext = name.rsplit_once('.')?.1;
    OFFER_EXTENSIONS.contains(&ext).then_some(url)
}

#[cfg(windows)]
fn clipboard_sequence() -> u32 {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetClipboardSequenceNumber() -> u32;
    }
    // SAFETY: takes no arguments and only reads a counter.
    unsafe { GetClipboardSequenceNumber() }
}

#[cfg(not(windows))]
fn clipboard_sequence() -> u32 {
    0
}

/// Watches the clipboard (only its change counter until something new is
/// copied) and offers new download links. Nothing is sent anywhere unless
/// the user clicks Download in the offer.
pub fn watch_clipboard(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last_seq = clipboard_sequence();
        let mut last_offered = String::new();
        loop {
            std::thread::sleep(CLIPBOARD_POLL);
            let seq = clipboard_sequence();
            if seq == last_seq {
                continue;
            }
            last_seq = seq;
            if !current_settings(&app).is_some_and(|s| s.offer_copied_links) {
                continue;
            }
            let Ok(text) = app.clipboard().read_text() else { continue };
            let Some(url) = offerable_link(&text) else { continue };
            if url.as_str() == last_offered || already_listed(&app, url.as_str()) {
                continue;
            }
            last_offered = url.to_string();
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || show_offer(&handle, url.to_string()));
        }
    });
}

/// Links already in the list (including ones BDM's own windows copy) aren't offered.
fn already_listed(app: &AppHandle, url: &str) -> bool {
    app.try_state::<AppState>()
        .and_then(|s| s.db().ok().and_then(|c| downloads::find_by_url(&c, url).ok().flatten()))
        .is_some()
}

fn show_offer(app: &AppHandle, url: String) {
    *app.state::<PendingOffer>().0.lock().unwrap_or_else(|e| e.into_inner()) = Some(url);
    if let Some(window) = app.get_webview_window(OFFER_WINDOW) {
        let _ = window.emit(EVENT_OFFER_CHANGED, ());
        let _ = window.show();
        return;
    }
    let mut builder = tauri::WebviewWindowBuilder::new(app, OFFER_WINDOW, tauri::WebviewUrl::App("index.html".into()))
        .title("Download with BDM?")
        .inner_size(OFFER_SIZE.0, OFFER_SIZE.1)
        .resizable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false);
    // Bottom-right corner, above the taskbar.
    if let Ok(Some(monitor)) = app.primary_monitor() {
        let scale = monitor.scale_factor();
        let area = monitor.work_area();
        let x = (area.position.x as f64 + area.size.width as f64) / scale - OFFER_SIZE.0 - OFFER_MARGIN;
        let y = (area.position.y as f64 + area.size.height as f64) / scale - OFFER_SIZE.1 - OFFER_MARGIN;
        builder = builder.position(x, y);
    }
    if let Err(e) = builder.build() {
        eprintln!("[bdm] couldn't show the copied-link offer: {e}");
    }
}

fn close_offer(app: &AppHandle) {
    *app.state::<PendingOffer>().0.lock().unwrap_or_else(|e| e.into_inner()) = None;
    if let Some(window) = app.get_webview_window(OFFER_WINDOW) {
        let _ = window.close();
    }
}

/// What the offer window shows.
#[tauri::command]
pub async fn get_clipboard_offer(offer: State<'_, PendingOffer>) -> AppResult<Option<ClipboardOffer>> {
    let url = offer.0.lock().map_err(|_| AppError::Internal("offer lock".into()))?.clone();
    Ok(url.and_then(|u| url::Url::parse(&u).ok()).map(|u| ClipboardOffer {
        file_name: validation::file_name_from_url(&u),
        host: u.host_str().unwrap_or_default().to_owned(),
        url: u.to_string(),
    }))
}

/// "Download": handled like a link sent by the browser (the New Download
/// dialog when confirmation is on).
#[tauri::command]
pub async fn accept_clipboard_offer(app: AppHandle) -> AppResult<()> {
    let url = app.state::<PendingOffer>().0.lock().map_err(|_| AppError::Internal("offer lock".into()))?.clone();
    close_offer(&app);
    let Some(url) = url else { return Ok(()) };
    let request = BrowserRequest::Download { url, file_name: None, referrer: None, mime: None, size: None, cookies: None };
    request.validate()?;
    let reply = crate::handle_browser_request(app.clone(), request).await;
    show_main_window(&app);
    match reply.error {
        Some(e) => Err(AppError::Validation(e)),
        None => Ok(()),
    }
}

#[tauri::command]
pub async fn dismiss_clipboard_offer(app: AppHandle) -> AppResult<()> {
    close_offer(&app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offers_only_single_download_links() {
        assert!(offerable_link("https://cdn.example.com/files/setup.exe").is_some());
        assert!(offerable_link("  https://e.com/a/Report.PDF?x=1 \n").is_some(), "trimmed, case-insensitive, query ok");
        assert!(offerable_link("https://e.com/page.html").is_none(), "not a download type");
        assert!(offerable_link("https://e.com/").is_none());
        assert!(offerable_link("see https://e.com/a.zip").is_none(), "only a bare link");
        assert!(offerable_link("https://user:pw@e.com/a.zip").is_none(), "credentials are refused");
        assert!(offerable_link("file:///C:/a.zip").is_none());
        assert!(offerable_link(&format!("https://e.com/{}.zip", "a".repeat(3000))).is_none());
    }

    #[test]
    fn tooltip_and_startup_command() {
        assert_eq!(tooltip_text(0, 0.0), APP_NAME);
        assert_eq!(tooltip_text(2, 1.3 * 1024.0 * 1024.0), format!("{APP_NAME}\n2 downloading · 1.3 MB/s"));
        assert_eq!(human_speed(512.0), "512 B/s");
        assert_eq!(
            run_command(std::path::Path::new(r"C:\Users\me\AppData\Local\Babarify Download Manager\bdm.exe")),
            r#""C:\Users\me\AppData\Local\Babarify Download Manager\bdm.exe" --minimized"#
        );
    }

    #[test]
    fn link_command_passes_the_link_as_one_argument() {
        assert_eq!(
            url_protocol_command(std::path::Path::new(r"C:\Program Files\BDM\bdm.exe")),
            r#""C:\Program Files\BDM\bdm.exe" "%1""#
        );
    }

    #[test]
    fn speeds_count_only_recent_reports() {
        let s = Speeds::default();
        s.record("a", 100.0);
        s.record("b", 50.0);
        assert_eq!(s.current(), (2, 150.0));
    }
}
