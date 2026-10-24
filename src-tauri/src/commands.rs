//! Typed Tauri commands — the only entry points the webview can call.
//!
//! Each command validates its own input; nothing here trusts the frontend.
//! TypeScript wrappers live in `src/lib/api.ts`.

use tauri::State;

use crate::db::{self, downloads, settings};
use crate::error::{AppError, AppResult};
use serde::Deserialize;

use crate::browser::registry::{self, BrowserStatus};
use crate::media::analyze::{self, MediaInfo};
use crate::media::args;
use crate::media::tools::{ToolKind, ToolStatus};
use crate::models::{
    AppInfo, AudioFormat, Container, Download, DownloadKind, MediaOptions, NewDownloadRequest,
    Settings, SettingsPatch,
};
use crate::state::AppState;
use crate::validation;

#[tauri::command]
pub async fn get_app_info(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<AppInfo> {
    let package = app.package_info();
    let schema_version = db::schema_version(&*state.db()?)?;
    Ok(AppInfo {
        name: package.name.clone(),
        version: package.version.to_string(),
        data_dir: state.data_dir.to_string_lossy().into_owned(),
        schema_version,
    })
}

#[tauri::command]
pub async fn list_downloads(state: State<'_, AppState>) -> AppResult<Vec<Download>> {
    downloads::list(&*state.db()?)
}

/// Validates the request, stores the download and (unless `start` is false)
/// queues it for transfer.
#[tauri::command]
pub async fn add_download(
    request: NewDownloadRequest,
    state: State<'_, AppState>,
) -> AppResult<Download> {
    let url = validation::validate_url(&request.url)?;
    let expected_sha256 = match request.expected_sha256.as_deref() {
        Some(raw) => validation::validate_sha256(raw)?,
        None => None,
    };

    let file_name = match request.file_name.as_deref().map(str::trim) {
        Some(name) if !name.is_empty() => validation::validate_file_name(name)?,
        _ => validation::file_name_from_url(&url),
    };

    let created = {
        let conn = state.db()?;
        let destination_dir = resolve_destination(&conn, &state, request.destination_dir.as_deref())?;
        downloads::insert(
            &conn,
            &downloads::InsertDownload {
                url: url.as_str(),
                file_name: &file_name,
                destination_dir: &destination_dir.to_string_lossy(),
                expected_sha256: expected_sha256.as_deref(),
                kind: DownloadKind::File,
                title: None,
                media_options: None,
                referrer: None,
            },
        )?
    };

    if request.start.unwrap_or(true) {
        state.engine.enqueue(&created.id)?;
        return downloads::require(&*state.db()?, &created.id);
    }
    Ok(created)
}

/// Starts a pending download, or resumes a paused, failed or cancelled one.
#[tauri::command]
pub async fn resume_download(id: String, state: State<'_, AppState>) -> AppResult<()> {
    state.engine.enqueue(validate_id(&id)?)
}

#[tauri::command]
pub async fn pause_download(id: String, state: State<'_, AppState>) -> AppResult<()> {
    state.engine.pause(validate_id(&id)?)
}

/// Stops a download and deletes its partial file.
#[tauri::command]
pub async fn cancel_download(id: String, state: State<'_, AppState>) -> AppResult<()> {
    state.engine.cancel(validate_id(&id)?)
}

/// Removes a download from the list. Partial files are deleted; finished
/// files are never touched.
#[tauri::command]
pub async fn remove_download(id: String, state: State<'_, AppState>) -> AppResult<()> {
    state.engine.remove(validate_id(&id)?)
}

/// Suggests a filename for a URL so the Add dialog can prefill it.
#[tauri::command]
pub async fn suggest_file_name(url: String) -> AppResult<String> {
    let url = validation::validate_url(&url)?;
    Ok(validation::file_name_from_url(&url))
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> AppResult<Settings> {
    settings::load(&*state.db()?, &state.fallback_download_dir)
}

#[tauri::command]
pub async fn update_settings(
    patch: SettingsPatch,
    state: State<'_, AppState>,
) -> AppResult<Settings> {
    let updated = {
        let mut conn = state.db()?;
        settings::apply_patch(&mut conn, &patch)?;
        settings::load(&conn, &state.fallback_download_dir)?
    };
    state
        .engine
        .set_limits(updated.max_concurrent_downloads, updated.segments_per_download)?;
    Ok(updated)
}

/// Status of yt-dlp, FFmpeg and Deno.
#[tauri::command]
pub async fn get_media_tools(state: State<'_, AppState>) -> AppResult<Vec<ToolStatus>> {
    Ok(state.tools.status().await)
}

/// Installs or updates BDM's own copies of the media tools (all of them when
/// `kinds` is omitted). Progress arrives as `tools:progress` events.
#[tauri::command]
pub async fn install_media_tools(
    kinds: Option<Vec<ToolKind>>,
    state: State<'_, AppState>,
) -> AppResult<Vec<ToolStatus>> {
    let kinds = kinds.unwrap_or_else(|| ToolKind::ALL.to_vec());
    state.tools.install(&kinds).await?;
    Ok(state.tools.status().await)
}

/// Which of BDM's own tool copies have a newer release available.
#[tauri::command]
pub async fn check_media_tool_updates(state: State<'_, AppState>) -> AppResult<Vec<ToolKind>> {
    let mut outdated = Vec::new();
    for kind in ToolKind::ALL {
        if state.tools.update_available(kind).await? {
            outdated.push(kind);
        }
    }
    Ok(outdated)
}

/// Lists the formats, qualities or playlist entries available for a URL.
#[tauri::command]
pub async fn analyze_media(url: String, state: State<'_, AppState>) -> AppResult<MediaInfo> {
    let url = validation::validate_url(&url)?;
    let tools = state.tools.require()?;
    analyze::analyze(&tools, url.as_str()).await
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MediaItem {
    pub url: String,
    pub title: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewMediaRequest {
    /// One item for a single video, several for selected playlist entries.
    pub items: Vec<MediaItem>,
    pub options: MediaOptions,
    pub destination_dir: Option<String>,
    pub start: Option<bool>,
}

/// Adds one media download per item, all with the same options.
#[tauri::command]
pub async fn add_media_downloads(
    request: NewMediaRequest,
    state: State<'_, AppState>,
) -> AppResult<Vec<Download>> {
    if request.items.is_empty() {
        return Err(AppError::Validation("Choose at least one video.".into()));
    }
    if request.items.len() > analyze::MAX_PLAYLIST_ENTRIES {
        return Err(AppError::Validation("Too many items at once.".into()));
    }
    args::validate(&request.options)?;
    let urls = request
        .items
        .iter()
        .map(|i| validation::validate_url(&i.url))
        .collect::<AppResult<Vec<_>>>()?;
    let ext = expected_extension(&request.options);

    let created = {
        let conn = state.db()?;
        let destination_dir = resolve_destination(&conn, &state, request.destination_dir.as_deref())?;
        let dest = destination_dir.to_string_lossy();
        let mut created = Vec::with_capacity(urls.len());
        for (item, url) in request.items.iter().zip(&urls) {
            let title = item.title.as_deref().map(str::trim).filter(|t| !t.is_empty());
            let base = title.map(validation::sanitize_file_name).unwrap_or_else(|| validation::file_name_from_url(url));
            let file_name = format!("{base}.{ext}");
            created.push(downloads::insert(
                &conn,
                &downloads::InsertDownload {
                    url: url.as_str(),
                    file_name: &file_name,
                    destination_dir: &dest,
                    expected_sha256: None,
                    kind: DownloadKind::Media,
                    title,
                    media_options: Some(&request.options),
                    referrer: None,
                },
            )?);
        }
        created
    };

    if request.start.unwrap_or(true) {
        for d in &created {
            state.engine.enqueue(&d.id)?;
        }
        let conn = state.db()?;
        return created.iter().map(|d| downloads::require(&conn, &d.id)).collect();
    }
    Ok(created)
}

/// Best guess at the final extension, shown until yt-dlp reports the real file.
fn expected_extension(options: &MediaOptions) -> &'static str {
    match options {
        MediaOptions::Video { container: Container::Mp4, .. } | MediaOptions::Format { container: Container::Mp4, .. } => "mp4",
        MediaOptions::Video { container: Container::Mkv, .. } | MediaOptions::Format { container: Container::Mkv, .. } => "mkv",
        MediaOptions::Audio { format: AudioFormat::Mp3 } => "mp3",
        MediaOptions::Audio { format: AudioFormat::M4a | AudioFormat::Best } => "m4a",
        MediaOptions::Audio { format: AudioFormat::Opus } => "opus",
    }
}

pub(crate) fn resolve_destination(
    conn: &rusqlite::Connection,
    state: &AppState,
    requested: Option<&str>,
) -> AppResult<std::path::PathBuf> {
    let dir = match requested.map(str::trim) {
        Some(dir) if !dir.is_empty() => dir.to_string(),
        _ => settings::load(conn, &state.fallback_download_dir)?.default_download_dir,
    };
    validation::validate_destination_dir(&dir)
}

/// Whether the native-messaging host is registered with Chrome and Edge.
#[tauri::command]
pub async fn get_browser_integration(state: State<'_, AppState>) -> AppResult<Vec<BrowserStatus>> {
    Ok(registry::status(&state.data_dir))
}

/// Registers the native-messaging host for the current user (no admin rights needed).
#[tauri::command]
pub async fn register_browser_integration(state: State<'_, AppState>) -> AppResult<Vec<BrowserStatus>> {
    registry::register(&state.data_dir, &registry::host_exe_path()?)?;
    Ok(registry::status(&state.data_dir))
}

#[tauri::command]
pub async fn unregister_browser_integration(state: State<'_, AppState>) -> AppResult<Vec<BrowserStatus>> {
    registry::unregister(&state.data_dir)?;
    Ok(registry::status(&state.data_dir))
}

/// Adds a download sent by the browser extension and starts it.
pub(crate) fn add_from_browser(
    state: &AppState,
    url: &str,
    file_name: Option<&str>,
    referrer: Option<&str>,
) -> AppResult<Download> {
    let url = validation::validate_url(url)?;
    let referrer = referrer
        .filter(|r| !r.trim().is_empty())
        .map(validation::validate_url)
        .transpose()?;
    // Browsers may report a full path; only the name is used.
    let file_name = match file_name.map(|n| n.rsplit(['/', '\\']).next().unwrap_or_default().trim()) {
        Some(name) if !name.is_empty() => validation::sanitize_file_name(name),
        _ => validation::file_name_from_url(&url),
    };
    let created = {
        let conn = state.db()?;
        let destination_dir = resolve_destination(&conn, state, None)?;
        downloads::insert(
            &conn,
            &downloads::InsertDownload {
                url: url.as_str(),
                file_name: &file_name,
                destination_dir: &destination_dir.to_string_lossy(),
                expected_sha256: None,
                kind: DownloadKind::File,
                title: None,
                media_options: None,
                referrer: referrer.as_ref().map(url::Url::as_str),
            },
        )?
    };
    state.engine.enqueue(&created.id)?;
    downloads::require(&*state.db()?, &created.id)
}

fn validate_id(id: &str) -> AppResult<&str> {
    uuid::Uuid::parse_str(id)
        .map(|_| id)
        .map_err(|_| AppError::Validation("Invalid download id.".into()))
}
