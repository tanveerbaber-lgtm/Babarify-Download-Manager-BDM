//! Media integration tests.
//!
//! Tool-manager tests use a fake GitHub API on the local test server and
//! never touch the network. The download tests drive the real yt-dlp and
//! FFmpeg against locally generated clips; they're skipped (with a note)
//! when those tools aren't installed.


use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::test_server::{FileSpec, TestServer};
use sha2::Digest;

use super::analyze;
use super::tools::{ToolKind, ToolManager, ToolSource};
use crate::db::{self, Database, downloads};
use crate::engine::events::{EventSink, Progress, ToolProgress};
use crate::engine::{Engine, EngineConfig};
use crate::models::{AudioFormat, Container, Download, DownloadKind, DownloadStatus, MediaOptions};

#[derive(Default)]
struct Sink {
    progress: Mutex<Vec<Progress>>,
    tools: Mutex<Vec<ToolProgress>>,
}

impl EventSink for Sink {
    fn progress(&self, p: &Progress) {
        self.progress.lock().unwrap().push(p.clone());
    }
    fn updated(&self, _d: &Download) {}
    fn removed(&self, _id: &str) {}
    fn tool_progress(&self, p: &ToolProgress) {
        self.tools.lock().unwrap().push(p.clone());
    }
}

fn sha(data: &[u8]) -> String {
    hex::encode(sha2::Sha256::digest(data))
}

fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut w = zip::ZipWriter::new(&mut buf);
        for (name, data) in files {
            w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap();
    }
    buf.into_inner()
}

/// Publishes a fake "latest release" with one asset.
fn publish(server: &TestServer, repo: &str, tag: &str, asset_name: &str, data: Vec<u8>, digest: Option<String>) {
    let size = data.len();
    let file_key = format!("{}-{}", repo.replace('/', "_"), asset_name);
    server.state.add(&file_key, FileSpec::new(data));
    let digest = digest.map(|d| format!(r#","digest":"{d}""#)).unwrap_or_default();
    let body = format!(
        r#"{{"tag_name":"{tag}","assets":[{{"name":"{asset_name}","size":{size},"browser_download_url":"{}"{digest}}}]}}"#,
        server.url(&format!("/files/{file_key}"))
    );
    server.state.set_json(&format!("repos/{repo}/releases/latest"), body);
}

fn publish_all(server: &TestServer, ytdlp: &[u8]) {
    publish(server, "yt-dlp/yt-dlp", "2026.10.21", "yt-dlp.exe", ytdlp.to_vec(), Some(format!("sha256:{}", sha(ytdlp))));
    let ff = zip_of(&[("ffmpeg-n9.0/bin/ffmpeg.exe", b"FF"), ("ffmpeg-n9.0/bin/ffprobe.exe", b"FP"), ("ffmpeg-n9.0/bin/ffplay.exe", b"X")]);
    let ff_digest = format!("sha256:{}", sha(&ff));
    publish(server, "BtbN/FFmpeg-Builds", "latest", "ffmpeg-n9.0-latest-win64-gpl-9.0.zip", ff, Some(ff_digest));
    let deno = zip_of(&[("deno.exe", b"DENO")]);
    let deno_digest = format!("sha256:{}", sha(&deno));
    publish(server, "denoland/deno", "v2.9.8", "deno-x86_64-pc-windows-msvc.zip", deno, Some(deno_digest));
}

fn manager(dir: &Path, server: &TestServer, sink: &Arc<Sink>) -> ToolManager {
    ToolManager::for_tests(dir.join("tools"), reqwest::Client::new(), sink.clone(), server.url("/api"), false)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn installs_verified_tools_from_releases() {
    let server = TestServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let sink = Arc::new(Sink::default());
    publish_all(&server, b"yt-dlp v1");
    let tools = manager(dir.path(), &server, &sink);

    assert!(tools.require().unwrap_err().to_string().contains("yt-dlp and FFmpeg"));
    tools.install(&ToolKind::ALL).await.unwrap();

    let t = dir.path().join("tools");
    assert_eq!(std::fs::read(t.join("yt-dlp/yt-dlp.exe")).unwrap(), b"yt-dlp v1");
    assert_eq!(std::fs::read(t.join("ffmpeg/ffmpeg.exe")).unwrap(), b"FF");
    assert_eq!(std::fs::read(t.join("ffmpeg/ffprobe.exe")).unwrap(), b"FP");
    assert!(!t.join("ffmpeg/ffplay.exe").exists(), "only the needed executables are kept");
    assert_eq!(std::fs::read(t.join("deno/deno.exe")).unwrap(), b"DENO");
    assert!(!t.join(".staging").exists());
    assert_eq!(tools.locate(ToolKind::YtDlp).unwrap().1, ToolSource::Managed);
    assert!(tools.require().is_ok());
    let stages: Vec<String> = sink.tools.lock().unwrap().iter().map(|p| p.stage.clone()).collect();
    assert!(stages.contains(&"Verifying".to_string()) && stages.contains(&"Installed".to_string()));

    // Same release again: nothing to do. A new release: update available, then installed.
    assert!(!tools.update_available(ToolKind::YtDlp).await.unwrap());
    publish(&server, "yt-dlp/yt-dlp", "2026.11.01", "yt-dlp.exe", b"yt-dlp v2".to_vec(), Some(format!("sha256:{}", sha(b"yt-dlp v2"))));
    assert!(tools.update_available(ToolKind::YtDlp).await.unwrap());
    tools.install(&[ToolKind::YtDlp]).await.unwrap();
    assert_eq!(std::fs::read(t.join("yt-dlp/yt-dlp.exe")).unwrap(), b"yt-dlp v2");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejects_downloads_that_fail_verification() {
    let server = TestServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let sink = Arc::new(Sink::default());
    publish_all(&server, b"good");
    let tools = manager(dir.path(), &server, &sink);
    tools.install(&[ToolKind::YtDlp]).await.unwrap();

    // Tampered asset: the published digest doesn't match the bytes served.
    publish(&server, "yt-dlp/yt-dlp", "evil", "yt-dlp.exe", b"tampered".to_vec(), Some(format!("sha256:{}", sha(b"something else"))));
    let err = tools.install(&[ToolKind::YtDlp]).await.unwrap_err().to_string();
    assert!(err.contains("verification"), "{err}");
    assert_eq!(std::fs::read(dir.path().join("tools/yt-dlp/yt-dlp.exe")).unwrap(), b"good", "existing copy untouched");

    // No digest at all: refuse rather than install unverified code.
    publish(&server, "yt-dlp/yt-dlp", "nodigest", "yt-dlp.exe", b"unknown".to_vec(), None);
    let err = tools.install(&[ToolKind::YtDlp]).await.unwrap_err().to_string();
    assert!(err.contains("checksum"), "{err}");
}

// ---------------------------------------------------------------- real tools

struct Real {
    ffmpeg: PathBuf,
    ffprobe: PathBuf,
}

/// The installed yt-dlp and FFmpeg, or `None` (test skipped) if missing.
fn real_tools(sink: &Arc<Sink>, dir: &Path) -> Option<(Arc<ToolManager>, Real)> {
    let tools = Arc::new(ToolManager::for_tests(dir.join("tools"), reqwest::Client::new(), sink.clone(), "http://127.0.0.1:9".into(), true));
    let paths = match tools.require() {
        Ok(p) => p,
        Err(_) => {
            eprintln!("skipping: yt-dlp/FFmpeg not installed");
            return None;
        }
    };
    let ffprobe = paths.ffmpeg.with_file_name("ffprobe.exe");
    Some((tools, Real { ffmpeg: paths.ffmpeg, ffprobe }))
}

/// Generates an H.264/AAC MP4 test clip.
fn make_clip(real: &Real, seconds: u32, size: &str) -> Vec<u8> {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("clip.mp4");
    let status = std::process::Command::new(&real.ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i"])
        .arg(format!("testsrc=duration={seconds}:size={size}:rate=30"))
        .args(["-f", "lavfi", "-i"])
        .arg(format!("sine=frequency=440:duration={seconds}"))
        .args(["-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"])
        .arg(&out)
        .status()
        .unwrap();
    assert!(status.success());
    std::fs::read(out).unwrap()
}

/// `codec_type:codec_name` of each stream, via ffprobe.
fn streams(real: &Real, file: &Path) -> Vec<String> {
    let out = std::process::Command::new(&real.ffprobe)
        .args(["-v", "error", "-show_entries", "stream=codec_type,codec_name", "-of", "csv=p=0"])
        .arg(file)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).lines().map(|l| {
        let mut p = l.split(',');
        let name = p.next().unwrap_or_default();
        let kind = p.next().unwrap_or_default();
        format!("{kind}:{name}")
    }).collect()
}

struct MediaHarness {
    server: TestServer,
    db: Arc<Database>,
    engine: Engine,
    sink: Arc<Sink>,
    real: Real,
    dest: tempfile::TempDir,
    _db_dir: tempfile::TempDir,
}

impl MediaHarness {
    async fn new() -> Option<Self> {
        let db_dir = tempfile::tempdir().unwrap();
        let sink = Arc::new(Sink::default());
        let (tools, real) = real_tools(&sink, db_dir.path())?;
        let db = Arc::new(Database::new(db::open(&db_dir.path().join("m.sqlite3")).unwrap()));
        // The test server is on 127.0.0.1, which BDM blocks by default.
        let allow = crate::models::SettingsPatch { allow_local_network: Some(true), scan_downloads: Some(false), ..Default::default() };
        crate::db::settings::apply_patch(&mut db.lock().unwrap(), &allow).unwrap();
        let engine = Engine::new(db.clone(), sink.clone(), EngineConfig::default(), tokio::runtime::Handle::current(), tools).unwrap();
        Some(Self { server: TestServer::start().await, db, engine, sink, real, dest: tempfile::tempdir().unwrap(), _db_dir: db_dir })
    }

    fn add(&self, path: &str, options: MediaOptions) -> String {
        self.add_from_page(path, options, None)
    }

    /// Adds a media download that names the page it was found on.
    fn add_from_page(&self, path: &str, options: MediaOptions, referrer: Option<&str>) -> String {
        let id = downloads::insert(
            &self.db.lock().unwrap(),
            &downloads::InsertDownload {
                url: &self.server.url(path),
                file_name: "pending.mp4",
                destination_dir: &self.dest.path().to_string_lossy(),
                expected_sha256: None,
                kind: DownloadKind::Media,
                title: None,
                media_options: Some(&options),
                referrer,
                queue_id: "main",
                priority: 0,
                category: None,
                speed_limit: None,
            },
        )
        .unwrap()
        .id;
        self.engine.enqueue(&id).unwrap();
        id
    }

    fn get(&self, id: &str) -> Download {
        downloads::require(&self.db.lock().unwrap(), id).unwrap()
    }

    async fn wait(&self, id: &str, pred: impl Fn(&Download) -> bool) -> Download {
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            let d = self.get(id);
            if pred(&d) {
                return d;
            }
            assert!(Instant::now() < deadline, "timed out: {d:?}");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    async fn wait_done(&self, id: &str) -> Download {
        self.wait(id, |d| matches!(d.status, DownloadStatus::Completed | DownloadStatus::Failed | DownloadStatus::Cancelled)).await
    }

    fn leftovers(&self) -> Vec<String> {
        std::fs::read_dir(self.dest.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(".bdm-media-"))
            .collect()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn downloads_video_with_audio_and_never_overwrites() {
    let Some(h) = MediaHarness::new().await else { return };
    h.server.state.add("clip.mp4", FileSpec::new(make_clip(&h.real, 3, "320x240")).etag("c1"));

    let opts = MediaOptions::Video { max_height: None, container: Container::Mp4 };
    let id = h.add("/files/clip.mp4", opts.clone());
    let d = h.wait_done(&id).await;
    assert_eq!(d.status, DownloadStatus::Completed, "{:?}", d.error_message);
    let saved = PathBuf::from(d.saved_path.unwrap());
    assert_eq!(saved.parent().unwrap(), h.dest.path());
    assert_eq!(d.file_name, "pending.mp4", "the name chosen in BDM is kept");
    let s = streams(&h.real, &saved);
    assert!(s.contains(&"video:h264".to_string()) && s.contains(&"audio:aac".to_string()), "{s:?}");
    assert!(h.leftovers().is_empty(), "temp folder removed");

    // Same video again: kept side by side, never overwritten.
    let id2 = h.add("/files/clip.mp4", opts);
    let d2 = h.wait_done(&id2).await;
    assert_eq!(d2.status, DownloadStatus::Completed);
    assert_ne!(d2.saved_path, Some(saved.to_string_lossy().into_owned()));
    assert!(d2.file_name.contains("(1)"), "{}", d2.file_name);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn extracts_audio_to_mp3() {
    let Some(h) = MediaHarness::new().await else { return };
    h.server.state.add("song.mp4", FileSpec::new(make_clip(&h.real, 3, "160x120")));
    let id = h.add("/files/song.mp4", MediaOptions::Audio { format: AudioFormat::Mp3 });
    let d = h.wait_done(&id).await;
    assert_eq!(d.status, DownloadStatus::Completed, "{:?}", d.error_message);
    assert!(d.file_name.ends_with(".mp3"));
    assert_eq!(streams(&h.real, Path::new(&d.saved_path.unwrap())), ["audio:mp3"]);
    let stages: Vec<Option<String>> = h.sink.progress.lock().unwrap().iter().filter(|p| p.id == id).map(|p| p.stage.clone()).collect();
    assert!(stages.iter().any(|s| s.as_deref() == Some("Converting audio")), "{stages:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pause_resume_and_cancel_media() {
    let Some(h) = MediaHarness::new().await else { return };
    let clip = make_clip(&h.real, 20, "640x360");
    let len = clip.len();
    // Slow enough that the pause lands mid-file even when the whole suite runs at once.
    h.server.state.add("long.mp4", FileSpec::new(clip.clone()).etag("l").delay(Duration::from_millis(120)));

    let id = h.add("/files/long.mp4", MediaOptions::Video { max_height: None, container: Container::Mp4 });
    h.wait(&id, |d| d.status == DownloadStatus::Downloading).await;
    let deadline = Instant::now() + Duration::from_secs(60);
    while !h.sink.progress.lock().unwrap().iter().any(|p| p.id == id && p.downloaded_bytes > 0) {
        assert!(Instant::now() < deadline, "no media progress");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    h.engine.pause(&id).unwrap();
    let paused = h.wait(&id, |d| d.status == DownloadStatus::Paused).await;
    assert!((paused.downloaded_bytes as usize) < len, "{paused:?}");
    assert_eq!(h.leftovers().len(), 1, "temp folder kept while paused");

    h.server.state.clear_log();
    h.engine.enqueue(&id).unwrap();
    let d = h.wait_done(&id).await;
    assert_eq!(d.status, DownloadStatus::Completed, "{:?}", d.error_message);
    assert_eq!(streams(&h.real, Path::new(d.saved_path.as_deref().unwrap())).len(), 2);
    let log = h.server.state.requests_for("/files/long.mp4");
    let resumed_with_range = log.iter().any(|l| l.range.as_deref().is_some_and(|r| !r.starts_with("bytes=0-")));
    assert!(resumed_with_range, "yt-dlp should continue its partial file; ranges: {:?}", log.iter().map(|l| l.range.clone()).collect::<Vec<_>>());

    // Cancel removes the temp folder.
    let id2 = h.add("/files/long.mp4", MediaOptions::Video { max_height: None, container: Container::Mkv });
    h.wait(&id2, |d| d.status == DownloadStatus::Downloading).await;
    tokio::time::sleep(Duration::from_millis(800)).await;
    h.engine.cancel(&id2).unwrap();
    h.wait(&id2, |d| d.status == DownloadStatus::Cancelled).await;
    assert!(h.leftovers().is_empty(), "{:?}", h.leftovers());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unsupported_pages_fail_clearly() {
    let Some(h) = MediaHarness::new().await else { return };
    let id = h.add("/page/blog", MediaOptions::Video { max_height: None, container: Container::Mp4 });
    let d = h.wait_done(&id).await;
    assert_eq!(d.status, DownloadStatus::Failed);
    let msg = d.error_message.unwrap();
    assert!(msg.contains("isn't supported"), "{msg}");
    assert!(!msg.contains("127.0.0.1"), "no URLs in messages: {msg}");
    assert!(h.leftovers().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn analyzes_a_direct_media_url() {
    let Some(h) = MediaHarness::new().await else { return };
    h.server.state.add("a.mp4", FileSpec::new(make_clip(&h.real, 2, "160x120")));
    let tools = h.engine_tools();
    let info = analyze::analyze(&tools, &h.server.url("/files/a.mp4"), None, &[], None).await.unwrap();
    assert_eq!(info.kind, "video");
    assert!(!info.formats.is_empty());
    assert!(!info.drm_protected);
}

/// Cuts a test clip into an HLS stream: `index.m3u8` plus `seg*.ts` files.
fn make_hls(real: &Real, seconds: u32) -> Vec<(String, Vec<u8>)> {
    let dir = tempfile::tempdir().unwrap();
    let clip = dir.path().join("clip.mp4");
    std::fs::write(&clip, make_clip(real, seconds, "320x240")).unwrap();
    let status = std::process::Command::new(&real.ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(&clip)
        .args(["-c", "copy", "-f", "hls", "-hls_time", "1", "-hls_playlist_type", "vod", "-hls_segment_filename"])
        .arg(dir.path().join("seg%d.ts"))
        .arg(dir.path().join("index.m3u8"))
        .status()
        .unwrap();
    assert!(status.success());
    std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n != "clip.mp4")
        .map(|n| {
            let data = std::fs::read(dir.path().join(&n)).unwrap();
            (n, data)
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reads_and_downloads_a_player_stream_with_the_page_as_referrer() {
    let Some(h) = MediaHarness::new().await else { return };
    for (name, data) in make_hls(&h.real, 3) {
        h.server.state.add(&name, FileSpec::new(data));
    }
    let tools = h.engine_tools();
    let stream = h.server.url("/guarded/index.m3u8");
    let page = h.server.url("/page/watch");

    // Like many CDNs, the test server refuses the stream without its page.
    assert!(analyze::analyze(&tools, &stream, None, &[], None).await.is_err());
    let info = analyze::analyze(&tools, &stream, Some(&page), &[], None).await.unwrap();
    let choices = super::choices::media_choices(&info).unwrap();
    assert!(choices.choices.iter().any(|c| c.kind == "video"), "{:?}", choices.choices);

    let opts = MediaOptions::Video { max_height: None, container: Container::Mp4 };
    let id = h.add_from_page("/guarded/index.m3u8", opts, Some(&page));
    let d = h.wait_done(&id).await;
    assert_eq!(d.status, DownloadStatus::Completed, "{:?}", d.error_message);
    let s = streams(&h.real, Path::new(&d.saved_path.unwrap()));
    assert!(s.contains(&"video:h264".to_string()), "{s:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn repeats_the_browsers_request_headers_for_a_detected_stream() {
    let Some(h) = MediaHarness::new().await else { return };
    for (name, data) in make_hls(&h.real, 3) {
        h.server.state.add(&name, FileSpec::new(data));
    }
    let tools = h.engine_tools();
    let stream = h.server.url("/tokened/index.m3u8");
    let headers = vec![("X-Stream-Token".to_string(), "ok".to_string()), ("Origin".to_string(), "http://page.test".to_string())];

    // The server wants the header the page's player added, on the playlist and every segment.
    assert!(analyze::analyze(&tools, &stream, None, &[], None).await.is_err());
    let info = analyze::analyze(&tools, &stream, None, &headers, None).await.unwrap();
    assert!(!info.formats.is_empty());

    h.engine.request_headers().remember(&stream, headers);
    let id = h.add("/tokened/index.m3u8", MediaOptions::Video { max_height: None, container: Container::Mp4 });
    let d = h.wait_done(&id).await;
    assert_eq!(d.status, DownloadStatus::Completed, "{:?}", d.error_message);
    assert!(streams(&h.real, Path::new(&d.saved_path.unwrap())).contains(&"video:h264".to_string()));
}

impl MediaHarness {
    fn engine_tools(&self) -> super::tools::ToolPaths {
        ToolManager::for_tests(PathBuf::from("unused"), reqwest::Client::new(), self.sink.clone(), String::new(), true)
            .require()
            .unwrap()
    }
}
