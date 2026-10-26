//! Desktop notifications and what happens when every download has finished.

use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::db::settings;
use crate::models::{Download, Settings, SettingsPatch, WhenDone};
use crate::state::AppState;

/// Seconds Windows waits before shutting down, so it can still be cancelled.
const SHUTDOWN_DELAY_SECS: u32 = 60;

fn current_settings(app: &AppHandle) -> Option<Settings> {
    let state = app.try_state::<AppState>()?;
    let conn = state.db().ok()?;
    settings::load(&conn, &state.fallback_download_dir).ok()
}

fn notify(app: &AppHandle, title: &str, body: &str) {
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        eprintln!("[bdm] couldn't show a notification: {e}");
    }
}

fn display_name(d: &Download) -> &str {
    d.title.as_deref().filter(|t| !t.is_empty()).unwrap_or(&d.file_name)
}

pub fn on_completed(app: &AppHandle, d: &Download) {
    if current_settings(app).is_some_and(|s| s.notify_on_complete) {
        notify(app, "Download Complete", display_name(d));
    }
}

pub fn on_failed(app: &AppHandle, d: &Download) {
    if current_settings(app).is_some_and(|s| s.notify_on_failure) {
        let reason = d.error_message.as_deref().unwrap_or("The download failed.");
        notify(app, "Download Failed", &format!("{}\n{reason}", display_name(d)));
    }
}

/// Always shown: a threat matters even with failure notifications off.
pub fn on_threat(app: &AppHandle, d: &Download) {
    notify(
        app,
        "Threat Found",
        &format!("{}\nMicrosoft Defender found a threat in this download and dealt with it. The file may have been removed.", display_name(d)),
    );
}

/// Runs the "when all downloads are done" action. It runs once, then the
/// setting goes back to "Do nothing" so the PC isn't shut down again later.
pub fn on_all_done(app: &AppHandle) {
    let Some(s) = current_settings(app) else { return };
    if s.when_all_done == WhenDone::Nothing {
        return;
    }
    if let Some(state) = app.try_state::<AppState>()
        && let Ok(mut conn) = state.db()
    {
        let reset = SettingsPatch { when_all_done: Some(WhenDone::Nothing), ..Default::default() };
        let _ = settings::apply_patch(&mut conn, &reset);
    }
    match s.when_all_done {
        WhenDone::Nothing => {}
        WhenDone::Exit => {
            notify(app, "All Downloads Finished", "Babarify Download Manager is closing.");
            app.exit(0);
        }
        WhenDone::Sleep => {
            notify(app, "All Downloads Finished", "Putting the PC to sleep.");
            sleep_now();
        }
        WhenDone::Shutdown => match shutdown_soon() {
            Ok(()) => notify(
                app,
                "All Downloads Finished",
                &format!("Windows will shut down in {SHUTDOWN_DELAY_SECS} seconds. Run \"shutdown /a\" to cancel."),
            ),
            Err(e) => notify(app, "Couldn't Shut Down", &e),
        },
    }
}

#[cfg(windows)]
fn sleep_now() {
    #[link(name = "powrprof")]
    unsafe extern "system" {
        fn SetSuspendState(hibernate: u8, force: u8, wakeup_events_disabled: u8) -> u8;
    }
    // SAFETY: plain Win32 call with no pointers; FALSE = sleep (not hibernate), not forced.
    unsafe {
        SetSuspendState(0, 0, 0);
    }
}

#[cfg(not(windows))]
fn sleep_now() {}

fn shutdown_soon() -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let system = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        let exe = std::path::Path::new(&system).join("System32").join("shutdown.exe");
        let status = std::process::Command::new(exe)
            .args(["/s", "/t", &SHUTDOWN_DELAY_SECS.to_string(), "/c", "Babarify Download Manager finished all downloads."])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map_err(|e| format!("Couldn't start shutdown: {e}"))?;
        if !status.success() {
            return Err("Windows refused the shutdown request.".into());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    Err("Shutting down is only supported on Windows.".into())
}
