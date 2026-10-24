//! Locating, installing and updating the external media tools.
//!
//! BDM prefers its own copies in `<data dir>/tools` and falls back to copies
//! on PATH. Installs come from each project's latest official GitHub release
//! and are only accepted if their SHA-256 matches the digest GitHub publishes
//! for the asset.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::process;
use crate::engine::events::{EventSink, ToolProgress};
use crate::error::{AppError, AppResult};

pub const GITHUB_API: &str = "https://api.github.com";
const VERSION_TIMEOUT: Duration = Duration::from_secs(15);
const MANIFEST: &str = "manifest.json";
const LAST_CHECK: &str = "last-update-check";
const AUTO_UPDATE_INTERVAL_MS: i64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolKind {
    YtDlp,
    Ffmpeg,
    Deno,
}

impl ToolKind {
    pub const ALL: [ToolKind; 3] = [ToolKind::YtDlp, ToolKind::Ffmpeg, ToolKind::Deno];

    pub fn exe(self) -> &'static str {
        match self {
            Self::YtDlp => "yt-dlp.exe",
            Self::Ffmpeg => "ffmpeg.exe",
            Self::Deno => "deno.exe",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::YtDlp => "yt-dlp",
            Self::Ffmpeg => "FFmpeg",
            Self::Deno => "Deno",
        }
    }

    fn dir_name(self) -> &'static str {
        match self {
            Self::YtDlp => "yt-dlp",
            Self::Ffmpeg => "ffmpeg",
            Self::Deno => "deno",
        }
    }

    fn repo(self) -> &'static str {
        match self {
            Self::YtDlp => "yt-dlp/yt-dlp",
            Self::Ffmpeg => "BtbN/FFmpeg-Builds",
            Self::Deno => "denoland/deno",
        }
    }

    /// What it's for, shown in Settings.
    pub fn purpose(self) -> &'static str {
        match self {
            Self::YtDlp => "Finds and downloads video and audio from supported sites.",
            Self::Ffmpeg => "Merges separate video and audio streams and converts audio.",
            Self::Deno => "JavaScript runtime yt-dlp needs for some sites, such as YouTube.",
        }
    }

    /// yt-dlp and FFmpeg are required; Deno only for some sites.
    pub fn required(self) -> bool {
        !matches!(self, Self::Deno)
    }

    /// Files to keep from the release asset (exe name → archive suffix).
    fn wanted_files(self) -> &'static [&'static str] {
        match self {
            Self::YtDlp => &["yt-dlp.exe"],
            Self::Ffmpeg => &["ffmpeg.exe", "ffprobe.exe"],
            Self::Deno => &["deno.exe"],
        }
    }

    /// Picks the Windows x64 asset from a release.
    fn pick_asset(self, assets: &[Asset]) -> Option<&Asset> {
        match self {
            Self::YtDlp => assets.iter().find(|a| a.name == "yt-dlp.exe"),
            Self::Deno => assets.iter().find(|a| a.name == "deno-x86_64-pc-windows-msvc.zip"),
            // Static GPL build of the newest release branch, e.g.
            // "ffmpeg-n9.0-latest-win64-gpl-9.0.zip".
            Self::Ffmpeg => assets
                .iter()
                .filter_map(|a| ffmpeg_branch(&a.name).map(|v| (v, a)))
                .max_by_key(|(v, _)| *v)
                .map(|(_, a)| a),
        }
    }
}

fn ffmpeg_branch(name: &str) -> Option<(u32, u32)> {
    let rest = name.strip_prefix("ffmpeg-n")?;
    if !name.ends_with(".zip") || !name.contains("-latest-win64-gpl-") || name.contains("shared") {
        return None;
    }
    let version = rest.split('-').next()?;
    let (major, minor) = version.split_once('.')?;
    Some((major.parse().ok()?, minor.parse().ok()?))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolSource {
    Managed,
    System,
    Missing,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub kind: ToolKind,
    pub name: &'static str,
    pub purpose: &'static str,
    pub required: bool,
    pub source: ToolSource,
    pub path: Option<String>,
    pub version: Option<String>,
}

/// Paths of the tools a media download needs.
#[derive(Debug, Clone)]
pub struct ToolPaths {
    pub yt_dlp: PathBuf,
    pub ffmpeg: PathBuf,
    pub deno: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Debug, Clone, Deserialize)]
struct Asset {
    name: String,
    size: u64,
    browser_download_url: String,
    digest: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Manifest {
    tools: HashMap<ToolKind, InstalledTool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstalledTool {
    release: String,
    asset: String,
    sha256: String,
    installed_at: i64,
}

pub struct ToolManager {
    tools_dir: PathBuf,
    client: reqwest::Client,
    api_base: String,
    sink: Arc<dyn EventSink>,
    /// One install at a time.
    install_lock: tokio::sync::Mutex<()>,
    /// Whether to look on PATH (tests turn this off to stay hermetic).
    search_system: bool,
}

impl ToolManager {
    pub fn new(tools_dir: PathBuf, client: reqwest::Client, sink: Arc<dyn EventSink>) -> Self {
        Self {
            tools_dir,
            client,
            api_base: GITHUB_API.to_string(),
            sink,
            install_lock: tokio::sync::Mutex::new(()),
            search_system: true,
        }
    }

    #[cfg(test)]
    pub fn for_tests(
        tools_dir: PathBuf,
        client: reqwest::Client,
        sink: Arc<dyn EventSink>,
        api_base: String,
        search_system: bool,
    ) -> Self {
        Self { api_base, search_system, ..Self::new(tools_dir, client, sink) }
    }

    fn managed_path(&self, kind: ToolKind) -> PathBuf {
        self.tools_dir.join(kind.dir_name()).join(kind.exe())
    }

    /// BDM's own copy first, then PATH.
    pub fn locate(&self, kind: ToolKind) -> Option<(PathBuf, ToolSource)> {
        let managed = self.managed_path(kind);
        if managed.is_file() {
            return Some((managed, ToolSource::Managed));
        }
        if self.search_system {
            return find_on_path(kind.exe()).map(|p| (p, ToolSource::System));
        }
        None
    }

    pub async fn status(&self) -> Vec<ToolStatus> {
        // Version checks start a process each, so run them side by side.
        let checks = ToolKind::ALL.map(|kind| async move {
            let located = self.locate(kind);
            let version = match &located {
                Some((path, _)) => tool_version(kind, path).await,
                None => None,
            };
            ToolStatus {
                kind,
                name: kind.label(),
                purpose: kind.purpose(),
                required: kind.required(),
                source: located.as_ref().map_or(ToolSource::Missing, |(_, s)| *s),
                path: located.map(|(p, _)| p.to_string_lossy().into_owned()),
                version,
            }
        });
        futures_util::future::join_all(checks).await
    }

    /// Paths for a media download, or a clear error if something is missing.
    pub fn require(&self) -> AppResult<ToolPaths> {
        let missing: Vec<&str> = [ToolKind::YtDlp, ToolKind::Ffmpeg]
            .into_iter()
            .filter(|k| self.locate(*k).is_none())
            .map(ToolKind::label)
            .collect();
        if !missing.is_empty() {
            return Err(AppError::Validation(format!(
                "Video downloads need {}. Install them in Settings → Media tools.",
                missing.join(" and ")
            )));
        }
        Ok(ToolPaths {
            yt_dlp: self.locate(ToolKind::YtDlp).map(|(p, _)| p).expect("checked"),
            ffmpeg: self.locate(ToolKind::Ffmpeg).map(|(p, _)| p).expect("checked"),
            deno: self.locate(ToolKind::Deno).map(|(p, _)| p),
        })
    }

    /// Installs or updates the given tools from their latest releases.
    /// Tools whose installed copy already matches the latest asset are skipped.
    pub async fn install(&self, kinds: &[ToolKind]) -> AppResult<()> {
        let _guard = self.install_lock.lock().await;
        std::fs::create_dir_all(&self.tools_dir)?;
        for &kind in kinds {
            self.install_one(kind).await.map_err(|e| match e {
                AppError::Validation(m) => AppError::Validation(m),
                other => AppError::Io(format!("Couldn't install {}: {other}", kind.label())),
            })?;
        }
        Ok(())
    }

    /// Updates BDM's own yt-dlp if it hasn't been checked in the last day.
    /// Copies on PATH are left to whoever installed them.
    pub async fn auto_update_yt_dlp(&self) -> AppResult<()> {
        if !self.read_manifest().tools.contains_key(&ToolKind::YtDlp) {
            return Ok(());
        }
        let stamp = self.tools_dir.join(LAST_CHECK);
        let last: i64 = std::fs::read_to_string(&stamp).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
        let now = crate::db::now_ms();
        if now - last < AUTO_UPDATE_INTERVAL_MS {
            return Ok(());
        }
        std::fs::write(&stamp, now.to_string())?;
        self.install(&[ToolKind::YtDlp]).await
    }

    /// True if BDM manages this tool and a newer release exists.
    pub async fn update_available(&self, kind: ToolKind) -> AppResult<bool> {
        let installed = self.read_manifest().tools.get(&kind).cloned();
        let Some(installed) = installed else { return Ok(false) };
        let release = self.latest_release(kind).await?;
        let asset = kind.pick_asset(&release.assets).ok_or_else(|| no_asset(kind))?;
        Ok(asset_sha256(asset)? != installed.sha256)
    }

    async fn install_one(&self, kind: ToolKind) -> AppResult<()> {
        let release = self.latest_release(kind).await?;
        let asset = kind.pick_asset(&release.assets).ok_or_else(|| no_asset(kind))?.clone();
        let expected = asset_sha256(&asset)?;

        let manifest = self.read_manifest();
        if manifest.tools.get(&kind).is_some_and(|t| t.sha256 == expected) && self.managed_path(kind).is_file() {
            return Ok(());
        }

        let staging = self.tools_dir.join(".staging");
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging)?;
        let download = staging.join(&asset.name);
        let actual = self.download_asset(kind, &asset, &download).await?;
        if actual != expected {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(AppError::Validation(format!(
                "{} download failed verification (SHA-256 mismatch). Nothing was installed.",
                kind.label()
            )));
        }

        self.emit(kind, asset.size, asset.size, "Installing");
        let staged_dir = staging.join("out");
        std::fs::create_dir_all(&staged_dir)?;
        let (src, dst, wanted) = (download.clone(), staged_dir.clone(), kind.wanted_files());
        tokio::task::spawn_blocking(move || extract(&src, &dst, wanted))
            .await
            .map_err(|e| AppError::Internal(e.to_string()))??;

        let target_dir = self.tools_dir.join(kind.dir_name());
        std::fs::create_dir_all(&target_dir)?;
        for name in wanted_names(kind) {
            replace_file(&staged_dir.join(name), &target_dir.join(name))?;
        }
        let _ = std::fs::remove_dir_all(&staging);

        let mut manifest = self.read_manifest();
        manifest.tools.insert(
            kind,
            InstalledTool { release: release.tag_name, asset: asset.name, sha256: expected, installed_at: crate::db::now_ms() },
        );
        std::fs::write(self.tools_dir.join(MANIFEST), serde_json::to_vec_pretty(&manifest)?)?;
        self.emit(kind, asset.size, asset.size, "Installed");
        Ok(())
    }

    async fn latest_release(&self, kind: ToolKind) -> AppResult<Release> {
        let url = format!("{}/repos/{}/releases/latest", self.api_base, kind.repo());
        let resp = self
            .client
            .get(&url)
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .send()
            .await
            .map_err(|e| AppError::Io(format!("couldn't reach GitHub: {}", e.without_url())))?;
        if !resp.status().is_success() {
            return Err(AppError::Io(format!("GitHub returned HTTP {} for the {} release", resp.status().as_u16(), kind.label())));
        }
        let body = resp
            .bytes()
            .await
            .map_err(|e| AppError::Io(format!("couldn't read the release data: {}", e.without_url())))?;
        serde_json::from_slice(&body).map_err(|e| AppError::Io(format!("unexpected release data: {e}")))
    }

    /// Streams the asset to `path` and returns its SHA-256.
    async fn download_asset(&self, kind: ToolKind, asset: &Asset, path: &Path) -> AppResult<String> {
        let resp = self
            .client
            .get(&asset.browser_download_url)
            .send()
            .await
            .map_err(|e| AppError::Io(format!("download failed: {}", e.without_url())))?;
        if !resp.status().is_success() {
            return Err(AppError::Io(format!("download failed with HTTP {}", resp.status().as_u16())));
        }
        let total = resp.content_length().unwrap_or(asset.size);
        let mut file = std::fs::File::create(path)?;
        let mut hasher = Sha256::new();
        let mut done = 0u64;
        let mut last = Instant::now();
        let mut stream = resp.bytes_stream();
        self.emit(kind, 0, total, "Downloading");
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| AppError::Io(format!("download interrupted: {}", e.without_url())))?;
            hasher.update(&chunk);
            file.write_all(&chunk)?;
            done += chunk.len() as u64;
            if last.elapsed() >= Duration::from_millis(250) {
                self.emit(kind, done, total, "Downloading");
                last = Instant::now();
            }
        }
        file.sync_all()?;
        self.emit(kind, done, total, "Verifying");
        Ok(hex::encode(hasher.finalize()))
    }

    fn emit(&self, kind: ToolKind, downloaded: u64, total: u64, stage: &str) {
        self.sink.tool_progress(&ToolProgress {
            tool: kind,
            downloaded_bytes: downloaded,
            total_bytes: total,
            stage: stage.to_string(),
        });
    }

    fn read_manifest(&self) -> Manifest {
        std::fs::read(self.tools_dir.join(MANIFEST))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }
}

fn no_asset(kind: ToolKind) -> AppError {
    AppError::Io(format!("the latest {} release has no Windows x64 download", kind.label()))
}

/// The digest GitHub publishes for an asset ("sha256:<hex>").
fn asset_sha256(asset: &Asset) -> AppResult<String> {
    asset
        .digest
        .as_deref()
        .and_then(|d| d.strip_prefix("sha256:"))
        .filter(|h| h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit()))
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| AppError::Validation(format!("{} has no published checksum, so it can't be verified. Nothing was installed.", asset.name)))
}

fn wanted_names(kind: ToolKind) -> impl Iterator<Item = &'static str> {
    kind.wanted_files().iter().copied()
}

/// Copies the wanted executables out of a zip (by file name, from any
/// folder), or takes a bare `.exe` asset as is.
fn extract(asset: &Path, out_dir: &Path, wanted: &[&str]) -> AppResult<()> {
    let is_zip = asset.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip"));
    if !is_zip {
        std::fs::copy(asset, out_dir.join(wanted[0]))?;
        return Ok(());
    }
    let file = std::fs::File::open(asset)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| AppError::Io(format!("bad archive: {e}")))?;
    let mut found = Vec::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| AppError::Io(format!("bad archive: {e}")))?;
        if !entry.is_file() {
            continue;
        }
        // Match on the last path component only; never use archive paths
        // for output, so entries can't escape `out_dir`.
        let full = entry.name().map_err(|e| AppError::Io(format!("bad archive: {e}")))?.into_owned();
        let base = full.rsplit(['/', '\\']).next().unwrap_or_default().to_string();
        if let Some(name) = wanted.iter().find(|w| w.eq_ignore_ascii_case(&base)) {
            let mut buf = Vec::with_capacity(entry.size() as usize);
            entry.read_to_end(&mut buf)?;
            std::fs::write(out_dir.join(name), buf)?;
            found.push(*name);
        }
    }
    let missing: Vec<_> = wanted.iter().filter(|w| !found.contains(w)).collect();
    if !missing.is_empty() {
        return Err(AppError::Io(format!("archive is missing {missing:?}")));
    }
    Ok(())
}

/// Puts `new` at `target`. A running exe can't be deleted on Windows but can
/// be renamed, so the old copy is moved aside first and cleaned up if possible.
fn replace_file(new: &Path, target: &Path) -> AppResult<()> {
    if target.exists() {
        let old = target.with_extension(format!("old-{}", crate::db::now_ms()));
        std::fs::rename(target, &old)?;
        let _ = std::fs::remove_file(&old);
    }
    std::fs::rename(new, target)?;
    Ok(())
}

/// Removes leftovers from earlier updates whose old exe was still running.
pub fn clean_old_copies(tools_dir: &Path) {
    for kind in ToolKind::ALL {
        if let Ok(entries) = std::fs::read_dir(tools_dir.join(kind.dir_name())) {
            for e in entries.flatten() {
                if e.file_name().to_string_lossy().contains(".old-") {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
    }
}

fn find_on_path(exe: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let mut dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    // winget's links folder is on PATH for new shells but may be missing from
    // an app started before winget ran.
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        dirs.push(PathBuf::from(local).join("Microsoft").join("WinGet").join("Links"));
    }
    dirs.into_iter().map(|d| d.join(exe)).find(|p| p.is_file())
}

async fn tool_version(kind: ToolKind, path: &Path) -> Option<String> {
    let (args, token): (&[&str], usize) = match kind {
        ToolKind::YtDlp => (&["--version"], 0),
        ToolKind::Ffmpeg => (&["-version"], 2),
        ToolKind::Deno => (&["--version"], 1),
    };
    let out = process::output(path, args, VERSION_TIMEOUT).await?;
    out.lines().next()?.split_whitespace().nth(token).map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str) -> Asset {
        Asset { name: name.into(), size: 1, browser_download_url: String::new(), digest: None }
    }

    #[test]
    fn picks_newest_static_gpl_ffmpeg() {
        let assets = vec![
            asset("ffmpeg-master-latest-win64-gpl.zip"),
            asset("ffmpeg-n8.1-latest-win64-gpl-8.1.zip"),
            asset("ffmpeg-n9.0-latest-win64-gpl-shared-9.0.zip"),
            asset("ffmpeg-n9.0-latest-win64-lgpl-9.0.zip"),
            asset("ffmpeg-n9.0-latest-win64-gpl-9.0.zip"),
            asset("ffmpeg-n9.0-latest-linux64-gpl-9.0.tar.xz"),
        ];
        assert_eq!(ToolKind::Ffmpeg.pick_asset(&assets).unwrap().name, "ffmpeg-n9.0-latest-win64-gpl-9.0.zip");
    }

    #[test]
    fn requires_a_valid_digest() {
        let mut a = asset("yt-dlp.exe");
        assert!(asset_sha256(&a).is_err());
        a.digest = Some("sha256:ABC".into());
        assert!(asset_sha256(&a).is_err());
        a.digest = Some(format!("sha256:{}", "A".repeat(64)));
        assert_eq!(asset_sha256(&a).unwrap(), "a".repeat(64));
    }

    #[test]
    fn extracts_only_wanted_files_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("t.zip");
        {
            let mut w = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
            let opts = zip::write::SimpleFileOptions::default();
            for (name, data) in [
                ("ffmpeg-n9.0/bin/ffmpeg.exe", "F"),
                ("ffmpeg-n9.0/bin/ffprobe.exe", "P"),
                ("ffmpeg-n9.0/bin/ffplay.exe", "X"),
                ("../../evil/ffmpeg.txt", "E"),
            ] {
                w.start_file(name, opts).unwrap();
                w.write_all(data.as_bytes()).unwrap();
            }
            w.finish().unwrap();
        }
        let out = dir.path().join("out");
        std::fs::create_dir(&out).unwrap();
        extract(&zip_path, &out, &["ffmpeg.exe", "ffprobe.exe"]).unwrap();
        let mut names: Vec<_> = std::fs::read_dir(&out).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        names.sort();
        assert_eq!(names, ["ffmpeg.exe", "ffprobe.exe"]);
        assert_eq!(std::fs::read_to_string(out.join("ffmpeg.exe")).unwrap(), "F");
        assert!(extract(&zip_path, &out, &["deno.exe"]).is_err());
    }
}
