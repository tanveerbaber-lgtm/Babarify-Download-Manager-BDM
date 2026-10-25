//! Typed Tauri commands — the only entry points the webview can call.
//!
//! Each command validates its own input; nothing here trusts the frontend.
//! TypeScript wrappers live in `src/lib/api.ts`.

use std::path::{Path, PathBuf};

use tauri::State;
use tauri_plugin_opener::OpenerExt;

use crate::batch;
use crate::db::{self, categories, downloads, queues, settings};
use crate::error::{AppError, AppResult};
use serde::Deserialize;

use crate::browser::registry::{self, BrowserStatus};
use crate::media::analyze::{self, MediaInfo};
use crate::media::args;
use crate::media::tools::{ToolKind, ToolStatus};
use crate::models::{
    AppInfo, AudioFormat, Category, Container, Download, DownloadKind, DownloadStatus, MediaOptions,
    NewDownloadRequest, Queue, QueueSchedule, Settings, SettingsPatch,
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

    let referrer = request
        .referrer
        .as_deref()
        .filter(|r| !r.trim().is_empty())
        .map(validation::validate_url)
        .transpose()?;
    let created = {
        let conn = state.db()?;
        let queue_id = queue_or_main(&conn, request.queue_id.as_deref())?;
        let (destination_dir, category) =
            placement_in(&conn, &state, request.destination_dir.as_deref(), &file_name, request.category.as_deref())?;
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
                referrer: referrer.as_ref().map(url::Url::as_str),
                queue_id: &queue_id,
                priority: request.priority.unwrap_or(0),
                category: category.as_deref(),
                speed_limit: kbps_to_bytes(request.speed_limit_kbps),
            },
        )?
    };

    if request.start.unwrap_or(true) {
        state.engine.enqueue(&created.id)?;
        return downloads::require(&*state.db()?, &created.id);
    }
    Ok(created)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BatchRequest {
    pub urls: Vec<String>,
    pub destination_dir: Option<String>,
    pub queue_id: Option<String>,
    pub start: Option<bool>,
}

/// Adds many links at once (from a pattern, a list or a .txt file).
#[tauri::command]
pub async fn add_batch(request: BatchRequest, state: State<'_, AppState>) -> AppResult<Vec<Download>> {
    if request.urls.is_empty() {
        return Err(AppError::Validation("Add at least one link.".into()));
    }
    if request.urls.len() > batch::MAX_BATCH {
        return Err(AppError::Validation(format!("Add at most {} links at once.", batch::MAX_BATCH)));
    }
    let urls = request.urls.iter().map(|u| validation::validate_url(u)).collect::<AppResult<Vec<_>>>()?;
    let created = {
        let conn = state.db()?;
        let queue_id = queue_or_main(&conn, request.queue_id.as_deref())?;
        // One transaction: a thousand links are written in one go.
        let tx = conn.unchecked_transaction()?;
        let mut created = Vec::with_capacity(urls.len());
        for url in &urls {
            let file_name = validation::file_name_from_url(url);
            let (dir, category) = placement(&conn, &state, request.destination_dir.as_deref(), &file_name)?;
            created.push(downloads::insert(
                &conn,
                &downloads::InsertDownload {
                    url: url.as_str(),
                    file_name: &file_name,
                    destination_dir: &dir.to_string_lossy(),
                    expected_sha256: None,
                    kind: DownloadKind::File,
                    title: None,
                    media_options: None,
                    referrer: None,
                    queue_id: &queue_id,
                    priority: 0,
                    category: category.as_deref(),
                    speed_limit: None,
                },
            )?);
        }
        tx.commit()?;
        created
    };
    if request.start.unwrap_or(true) {
        for d in &created {
            state.engine.enqueue(&d.id)?;
        }
    }
    let conn = state.db()?;
    created.iter().map(|d| downloads::require(&conn, &d.id)).collect()
}

/// Expands a numbered pattern like `file[01-20].zip`.
#[tauri::command]
pub async fn expand_batch_pattern(pattern: String) -> AppResult<Vec<String>> {
    let urls = batch::expand_pattern(&pattern)?;
    for u in &urls {
        validation::validate_url(u)?;
    }
    Ok(urls)
}

/// Reads the links from a text file the user picked.
#[tauri::command]
pub async fn import_url_list(path: String) -> AppResult<Vec<String>> {
    const MAX_SIZE: u64 = 2 * 1024 * 1024;
    let path = PathBuf::from(path.trim());
    let is_text = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("txt") || e.eq_ignore_ascii_case("csv"));
    if !path.is_absolute() || !is_text {
        return Err(AppError::Validation("Choose a .txt file with one link per line.".into()));
    }
    if std::fs::metadata(&path)?.len() > MAX_SIZE {
        return Err(AppError::Validation("That file is too large for a link list.".into()));
    }
    let bytes = std::fs::read(&path)?;
    let urls = batch::extract_urls(&String::from_utf8_lossy(&bytes));
    if urls.is_empty() {
        return Err(AppError::Validation("No http or https links were found in that file.".into()));
    }
    Ok(urls)
}

/// The most recent download of the same link, so the UI can warn before adding it again.
#[tauri::command]
pub async fn find_duplicate(url: String, state: State<'_, AppState>) -> AppResult<Option<Download>> {
    let url = validation::validate_url(&url)?;
    let found = downloads::find_by_url(&*state.db()?, url.as_str())?;
    Ok(found.filter(|d| d.status != DownloadStatus::Cancelled))
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

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BulkAction {
    Resume,
    Pause,
    Cancel,
    Remove,
}

/// Applies one action to many downloads in a single call. Every id is
/// attempted; the first error (if any) is returned for the UI to show.
#[tauri::command]
pub async fn bulk_action(action: BulkAction, ids: Vec<String>, state: State<'_, AppState>) -> AppResult<Option<String>> {
    validate_ids(&ids)?;
    let mut first_error = None;
    for id in &ids {
        let result = match action {
            BulkAction::Resume => state.engine.enqueue(id),
            BulkAction::Pause => state.engine.pause(id),
            BulkAction::Cancel => state.engine.cancel(id),
            BulkAction::Remove => state.engine.remove(id),
        };
        if let Err(e) = result {
            first_error.get_or_insert(e.to_string());
        }
    }
    Ok(first_error)
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
    state.engine.apply_policy(&updated)?;
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
    state.engine.net_guard().check_url(&url).await.map_err(|b| AppError::Validation(b.to_string()))?;
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
    /// Category id chosen in the dialog; Videos/Music by format when omitted.
    pub category: Option<String>,
}

/// Adds one media download per item, all with the same options.
#[tauri::command]
pub async fn add_media_downloads(
    request: NewMediaRequest,
    state: State<'_, AppState>,
) -> AppResult<Vec<Download>> {
    add_media_in(
        &state,
        &request.items,
        &request.options,
        request.destination_dir.as_deref(),
        request.start.unwrap_or(true),
        request.category.as_deref(),
    )
}

/// Shared by the Add dialog and the browser's one-click download.
pub(crate) fn add_media(
    state: &AppState,
    items: &[MediaItem],
    options: &MediaOptions,
    destination_dir: Option<&str>,
    start: bool,
) -> AppResult<Vec<Download>> {
    add_media_in(state, items, options, destination_dir, start, None)
}

pub(crate) fn add_media_in(
    state: &AppState,
    items: &[MediaItem],
    options: &MediaOptions,
    destination_dir: Option<&str>,
    start: bool,
    category: Option<&str>,
) -> AppResult<Vec<Download>> {
    if items.is_empty() {
        return Err(AppError::Validation("Choose at least one video.".into()));
    }
    if items.len() > analyze::MAX_PLAYLIST_ENTRIES {
        return Err(AppError::Validation("Too many items at once.".into()));
    }
    args::validate(options)?;
    let urls = items
        .iter()
        .map(|i| validation::validate_url(&i.url))
        .collect::<AppResult<Vec<_>>>()?;
    let ext = expected_extension(options);

    let created = {
        let conn = state.db()?;
        let (destination_dir, category) = placement_in(&conn, state, destination_dir, &format!("x.{ext}"), category)?;
        let dest = destination_dir.to_string_lossy();
        let mut created = Vec::with_capacity(urls.len());
        for (item, url) in items.iter().zip(&urls) {
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
                    media_options: Some(options),
                    referrer: None,
                    queue_id: queues::MAIN_QUEUE,
                    priority: 0,
                    category: category.as_deref(),
                    speed_limit: None,
                },
            )?);
        }
        created
    };

    if start {
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

/// Where a new download is saved and which category it's in: the folder
/// asked for, else its category's folder (when sorting is on), else the
/// default download folder.
pub(crate) fn placement(
    conn: &rusqlite::Connection,
    state: &AppState,
    requested: Option<&str>,
    file_name: &str,
) -> AppResult<(PathBuf, Option<String>)> {
    placement_in(conn, state, requested, file_name, None)
}

/// Like [`placement`], with a category the user picked (`"other"` = none).
pub(crate) fn placement_in(
    conn: &rusqlite::Connection,
    state: &AppState,
    requested: Option<&str>,
    file_name: &str,
    chosen_category: Option<&str>,
) -> AppResult<(PathBuf, Option<String>)> {
    let category = match chosen_category {
        Some("other") => None,
        Some(id) => Some(
            categories::list(conn)?
                .into_iter()
                .find(|c| c.id == id)
                .ok_or_else(|| AppError::Validation("That category no longer exists.".into()))?,
        ),
        None => categories::for_file_name(conn, file_name)?,
    };
    let id = category.as_ref().map(|c| c.id.clone());
    if let Some(dir) = requested.map(str::trim).filter(|d| !d.is_empty()) {
        // A new folder inside an existing one (e.g. "Downloads\Videos") is created.
        let path = Path::new(dir);
        if path.is_absolute() && !path.exists() && path.parent().is_some_and(Path::is_dir) {
            std::fs::create_dir(path)?;
        }
        return Ok((validation::validate_destination_dir(dir)?, id));
    }
    let s = settings::load(conn, &state.fallback_download_dir)?;
    let dir = match category.filter(|_| s.sort_into_categories) {
        Some(c) => {
            let base = validation::validate_destination_dir(&s.default_download_dir)?;
            let folder = categories::folder_for(&c, &base);
            std::fs::create_dir_all(&folder)?;
            folder
        }
        None => PathBuf::from(&s.default_download_dir),
    };
    Ok((validation::validate_destination_dir(&dir.to_string_lossy())?, id))
}

fn queue_or_main(conn: &rusqlite::Connection, requested: Option<&str>) -> AppResult<String> {
    match requested.map(str::trim).filter(|q| !q.is_empty()) {
        Some(q) => Ok(queues::require(conn, q)?.id),
        None => Ok(queues::MAIN_QUEUE.to_string()),
    }
}

fn kbps_to_bytes(kbps: Option<u32>) -> Option<i64> {
    kbps.filter(|k| *k > 0).map(|k| i64::from(k.min(settings::MAX_SPEED_LIMIT_KBPS)) * 1024)
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

/// What the "Download File Info" dialog needs for a request from the browser.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserProposal {
    /// "file" or "media".
    pub kind: &'static str,
    pub url: String,
    pub file_name: String,
    pub size: Option<u64>,
    pub referrer: Option<String>,
    /// Detected category id ("other" when none matches).
    pub category: String,
    pub title: Option<String>,
    pub options: Option<MediaOptions>,
    /// e.g. "MP4 · 1080p".
    pub label: Option<String>,
    /// A previous download of the same link, if any.
    pub duplicate: Option<Download>,
}

/// The file name to use for a browser download (browsers may send a full path).
fn browser_file_name(url: &url::Url, file_name: Option<&str>) -> String {
    match file_name.map(|n| n.rsplit(['/', '\\']).next().unwrap_or_default().trim()) {
        Some(name) if !name.is_empty() => validation::sanitize_file_name(name),
        _ => validation::file_name_from_url(url),
    }
}

/// Validates a browser file request and describes it for the confirm dialog.
pub(crate) fn propose_file(
    state: &AppState,
    url: &str,
    file_name: Option<&str>,
    referrer: Option<&str>,
    size: Option<i64>,
) -> AppResult<BrowserProposal> {
    let url = validation::validate_url(url)?;
    let referrer = referrer.filter(|r| !r.trim().is_empty()).map(validation::validate_url).transpose()?;
    let file_name = browser_file_name(&url, file_name);
    let conn = state.db()?;
    let category = categories::for_file_name(&conn, &file_name)?.map_or_else(|| "other".to_string(), |c| c.id);
    let duplicate = downloads::find_by_url(&conn, url.as_str())?.filter(|d| d.status != DownloadStatus::Cancelled);
    Ok(BrowserProposal {
        kind: "file",
        url: url.to_string(),
        file_name,
        size: size.and_then(|s| u64::try_from(s).ok()).filter(|s| *s > 0),
        referrer: referrer.map(|r| r.to_string()),
        category,
        title: None,
        options: None,
        label: None,
        duplicate,
    })
}

/// Describes a video/audio choice from the on-video panel for the confirm dialog.
pub(crate) fn propose_media(
    state: &AppState,
    url: &str,
    title: Option<&str>,
    options: &MediaOptions,
    label: Option<&str>,
    size: Option<u64>,
) -> AppResult<BrowserProposal> {
    let url = validation::validate_url(url)?;
    args::validate(options)?;
    let ext = expected_extension(options);
    let title = title.map(str::trim).filter(|t| !t.is_empty());
    let base = title.map(validation::sanitize_file_name).unwrap_or_else(|| validation::file_name_from_url(&url));
    let file_name = format!("{base}.{ext}");
    let conn = state.db()?;
    let category = categories::for_file_name(&conn, &file_name)?.map_or_else(|| "other".to_string(), |c| c.id);
    let duplicate = downloads::find_by_url(&conn, url.as_str())?.filter(|d| d.status != DownloadStatus::Cancelled);
    Ok(BrowserProposal {
        kind: "media",
        url: url.to_string(),
        file_name,
        size,
        referrer: None,
        category,
        title: title.map(str::to_owned),
        options: Some(options.clone()),
        label: label.map(str::to_owned),
        duplicate,
    })
}

/// Opens (or focuses) a download's own window.
#[tauri::command]
pub async fn open_download_window(id: String, app: tauri::AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    use tauri::Manager;
    let d = downloads::require(&*state.db()?, validate_id(&id)?)?;
    let label = format!("download-{}", d.id);
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.unminimize();
        let _ = w.set_focus();
        return Ok(());
    }
    tauri::WebviewWindowBuilder::new(&app, &label, tauri::WebviewUrl::App("index.html".into()))
        .title(d.title.as_deref().unwrap_or(&d.file_name))
        .inner_size(600.0, 560.0)
        .min_inner_size(480.0, 400.0)
        .resizable(true)
        .build()
        .map_err(|e| AppError::Internal(format!("couldn't open the download window: {e}")))?;
    Ok(())
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
    let file_name = browser_file_name(&url, file_name);
    let created = {
        let conn = state.db()?;
        let s = settings::load(&conn, &state.fallback_download_dir)?;
        if s.duplicate_action == crate::models::DuplicateAction::Skip
            && let Some(old) = downloads::find_by_url(&conn, url.as_str())?
            && old.status != DownloadStatus::Cancelled
        {
            return Err(AppError::Validation(format!("BDM already has \"{}\" in its list, so it was skipped.", old.file_name)));
        }
        let (destination_dir, category) = placement(&conn, state, None, &file_name)?;
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
                queue_id: queues::MAIN_QUEUE,
                priority: 0,
                category: category.as_deref(),
                speed_limit: None,
            },
        )?
    };
    state.engine.enqueue(&created.id)?;
    downloads::require(&*state.db()?, &created.id)
}

// ------------------------------------------------------------------ queues

#[tauri::command]
pub async fn list_queues(state: State<'_, AppState>) -> AppResult<Vec<Queue>> {
    queues::list(&*state.db()?)
}

/// Creates a queue (no `id`) or renames / reschedules one.
#[tauri::command]
pub async fn save_queue(
    id: Option<String>,
    name: String,
    schedule: QueueSchedule,
    state: State<'_, AppState>,
) -> AppResult<Queue> {
    let saved = queues::save(&*state.db()?, &name, &schedule, id.as_deref())?;
    state.engine.reschedule()?;
    Ok(saved)
}

/// Deletes a queue; its downloads move to Main.
#[tauri::command]
pub async fn delete_queue(id: String, state: State<'_, AppState>) -> AppResult<Vec<Download>> {
    if id == queues::MAIN_QUEUE {
        return Err(AppError::Validation("The Main queue can't be deleted.".into()));
    }
    state.engine.stop_queue(&id)?;
    queues::delete(&*state.db()?, &id)?;
    state.engine.reschedule()?;
    downloads::list(&*state.db()?)
}

#[tauri::command]
pub async fn start_queue(id: String, state: State<'_, AppState>) -> AppResult<Vec<Queue>> {
    state.engine.start_queue(&id)?;
    queues::list(&*state.db()?)
}

#[tauri::command]
pub async fn stop_queue(id: String, state: State<'_, AppState>) -> AppResult<Vec<Queue>> {
    state.engine.stop_queue(&id)?;
    queues::list(&*state.db()?)
}

// -------------------------------------------------------------- categories

#[tauri::command]
pub async fn list_categories(state: State<'_, AppState>) -> AppResult<Vec<Category>> {
    categories::list(&*state.db()?)
}

#[tauri::command]
pub async fn save_category(
    id: Option<String>,
    name: String,
    extensions: Vec<String>,
    folder: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<Category> {
    categories::save(&*state.db()?, id.as_deref(), &name, &extensions, folder.as_deref())
}

#[tauri::command]
pub async fn delete_category(id: String, state: State<'_, AppState>) -> AppResult<()> {
    categories::delete(&*state.db()?, &id)
}

// ------------------------------------------------------------ bulk actions

fn validate_ids(ids: &[String]) -> AppResult<()> {
    if ids.is_empty() || ids.len() > batch::MAX_BATCH {
        return Err(AppError::Validation("Select between 1 and 1000 downloads.".into()));
    }
    ids.iter().try_for_each(|id| validate_id(id).map(|_| ()))
}

/// Applies `f` to each id and returns the updated records.
fn for_each_id(state: &AppState, ids: &[String], mut f: impl FnMut(&str) -> AppResult<()>) -> AppResult<Vec<Download>> {
    validate_ids(ids)?;
    for id in ids {
        f(id)?;
    }
    let conn = state.db()?;
    ids.iter().filter_map(|id| downloads::get(&conn, id).transpose()).collect()
}

/// -1 low, 0 normal, 1 high.
#[tauri::command]
pub async fn set_download_priority(ids: Vec<String>, priority: i32, state: State<'_, AppState>) -> AppResult<Vec<Download>> {
    if !(-1..=1).contains(&priority) {
        return Err(AppError::Validation("Priority is low, normal or high.".into()));
    }
    let updated = for_each_id(&state, &ids, |id| downloads::set_priority(&*state.db()?, id, priority))?;
    state.engine.reschedule()?;
    Ok(updated)
}

#[tauri::command]
pub async fn move_downloads_to_queue(ids: Vec<String>, queue_id: String, state: State<'_, AppState>) -> AppResult<Vec<Download>> {
    for_each_id(&state, &ids, |id| state.engine.move_to_queue(id, &queue_id))
}

/// Moves downloads to the front (`true`) or back of the waiting order.
#[tauri::command]
pub async fn move_downloads_in_queue(ids: Vec<String>, front: bool, state: State<'_, AppState>) -> AppResult<Vec<Download>> {
    // Moving to the front goes last-to-first so the selection keeps its order.
    let ordered: Vec<String> = if front { ids.iter().rev().cloned().collect() } else { ids.clone() };
    for_each_id(&state, &ordered, |id| downloads::move_to_edge(&*state.db()?, id, front))?;
    state.engine.reschedule()?;
    let conn = state.db()?;
    ids.iter().filter_map(|id| downloads::get(&conn, id).transpose()).collect()
}

/// KB/s; 0 or `None` removes the download's own limit.
#[tauri::command]
pub async fn set_download_speed_limit(ids: Vec<String>, kbps: Option<u32>, state: State<'_, AppState>) -> AppResult<Vec<Download>> {
    let limit = kbps_to_bytes(kbps);
    for_each_id(&state, &ids, |id| state.engine.set_speed_limit(id, limit))
}

/// Downloads a finished (or failed) file again; the old file is kept.
#[tauri::command]
pub async fn redownload(ids: Vec<String>, state: State<'_, AppState>) -> AppResult<Vec<Download>> {
    for_each_id(&state, &ids, |id| {
        let status = downloads::require(&*state.db()?, id)?.status;
        if matches!(status, DownloadStatus::Downloading | DownloadStatus::Queued) {
            return Ok(());
        }
        if matches!(status, DownloadStatus::Completed | DownloadStatus::Failed) {
            downloads::prepare_redownload(&*state.db()?, id)?;
        }
        state.engine.enqueue(id)
    })
}

/// Removes completed and cancelled downloads from the list (files stay on disk).
#[tauri::command]
pub async fn clear_finished(state: State<'_, AppState>) -> AppResult<Vec<String>> {
    downloads::clear_finished(&*state.db()?)
}

fn saved_file(state: &AppState, id: &str) -> AppResult<PathBuf> {
    let d = downloads::require(&*state.db()?, validate_id(id)?)?;
    match d.saved_path.map(PathBuf::from) {
        Some(p) if d.status == DownloadStatus::Completed && p.is_file() => Ok(p),
        _ => Err(AppError::NotFound("The file isn't there any more. It may have been moved or deleted.".into())),
    }
}

/// SHA-256 of a finished file, to compare with a checksum the site publishes.
#[tauri::command]
pub async fn file_sha256(id: String, state: State<'_, AppState>) -> AppResult<String> {
    let path = saved_file(&state, &id)?;
    tokio::task::spawn_blocking(move || crate::engine::files::sha256_file(&path))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .map_err(AppError::from)
}

/// Opens a finished file with its default app.
#[tauri::command]
pub async fn open_download(id: String, app: tauri::AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    let path = saved_file(&state, &id)?;
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|e| AppError::Io(format!("couldn't open the file: {e}")))
}

/// Shows a finished file (or, failing that, the destination folder) in Explorer.
#[tauri::command]
pub async fn show_in_folder(id: String, app: tauri::AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    let result = match saved_file(&state, &id) {
        Ok(path) => app.opener().reveal_item_in_dir(&path),
        Err(_) => {
            let dir = downloads::require(&*state.db()?, validate_id(&id)?)?.destination_dir;
            if !Path::new(&dir).is_dir() {
                return Err(AppError::NotFound("That folder no longer exists.".into()));
            }
            app.opener().open_path(dir, None::<&str>)
        }
    };
    result.map_err(|e| AppError::Io(format!("couldn't open the folder: {e}")))
}

fn validate_id(id: &str) -> AppResult<&str> {
    uuid::Uuid::parse_str(id)
        .map(|_| id)
        .map_err(|_| AppError::Validation("Invalid download id.".into()))
}
