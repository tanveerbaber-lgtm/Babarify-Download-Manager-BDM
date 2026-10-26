//! Engine integration tests against a local HTTP server.


use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::test_server::{self as server, FileSpec, TestServer, test_data};

use super::events::{EventSink, Progress};
use super::files::TEMP_EXTENSION;
use super::{Engine, EngineConfig};
use crate::db::{self, Database, downloads, queues};
use crate::models::{Download, DownloadStatus, QueueSchedule};

#[derive(Default)]
struct RecordingSink {
    progress: Mutex<Vec<Progress>>,
    updates: Mutex<Vec<Download>>,
    removed: Mutex<Vec<String>>,
    completed: Mutex<Vec<String>>,
    failed: Mutex<Vec<String>>,
    all_done: std::sync::atomic::AtomicUsize,
}

impl EventSink for RecordingSink {
    fn progress(&self, p: &Progress) {
        self.progress.lock().unwrap().push(p.clone());
    }
    fn updated(&self, d: &Download) {
        self.updates.lock().unwrap().push(d.clone());
    }
    fn removed(&self, id: &str) {
        self.removed.lock().unwrap().push(id.to_string());
    }
    fn completed(&self, d: &Download) {
        self.completed.lock().unwrap().push(d.id.clone());
    }
    fn failed(&self, d: &Download) {
        self.failed.lock().unwrap().push(d.id.clone());
    }
    fn all_done(&self) {
        self.all_done.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

fn test_config() -> EngineConfig {
    EngineConfig {
        connect_timeout: Duration::from_secs(2),
        read_timeout: Duration::from_millis(800),
        retry_base_delay: Duration::from_millis(20),
        max_retry_delay: Duration::from_millis(200),
        min_segment_size: 64 * 1024,
        progress_interval: Duration::from_millis(50),
        persist_every_ticks: 2,
        ..EngineConfig::default()
    }
}

/// A tool manager that never touches the network or the real PATH.
fn test_tools(dir: &tempfile::TempDir, sink: &Arc<RecordingSink>) -> Arc<crate::media::tools::ToolManager> {
    Arc::new(crate::media::tools::ToolManager::for_tests(
        dir.path().join("tools"),
        reqwest::Client::new(),
        sink.clone(),
        "http://127.0.0.1:9".into(),
        false,
    ))
}

struct Harness {
    server: TestServer,
    db: Arc<Database>,
    sink: Arc<RecordingSink>,
    engine: Engine,
    dir: tempfile::TempDir,
    /// Keeps the database directory alive for the test's duration.
    _db_dir: tempfile::TempDir,
}

impl Harness {
    async fn new() -> Self {
        let server = TestServer::start().await;
        let db_dir = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::new(db::open(&db_dir.path().join("t.sqlite3")).unwrap()));
        // The test server is on 127.0.0.1, which BDM blocks by default.
        let allow = crate::models::SettingsPatch { allow_local_network: Some(true), ..Default::default() };
        crate::db::settings::apply_patch(&mut db.lock().unwrap(), &allow).unwrap();
        let sink = Arc::new(RecordingSink::default());
        let engine = Engine::new(db.clone(), sink.clone(), test_config(), tokio::runtime::Handle::current(), test_tools(&db_dir, &sink)).unwrap();
        engine.set_limits(3, 8).unwrap();
        Self { server, db, sink, engine, dir: tempfile::tempdir().unwrap(), _db_dir: db_dir }
    }

    /// Simulates an app restart: a new engine on the same database.
    fn restart_engine(&mut self) {
        self.engine = Engine::new(
            self.db.clone(),
            self.sink.clone(),
            test_config(),
            tokio::runtime::Handle::current(),
            test_tools(&self._db_dir, &self.sink),
        )
        .unwrap();
        self.engine.set_limits(3, 8).unwrap();
    }

    fn insert(&self, url: &str, file_name: &str, sha256: Option<&str>) -> String {
        let conn = self.db.lock().unwrap();
        downloads::insert(
            &conn,
            &downloads::InsertDownload {
                url,
                file_name,
                destination_dir: &self.dir.path().to_string_lossy(),
                expected_sha256: sha256,
                kind: crate::models::DownloadKind::File,
                title: None,
                media_options: None,
                referrer: None,
                queue_id: "main",
                priority: 0,
                category: None,
                speed_limit: None,
            },
        )
        .unwrap()
        .id
    }

    fn add(&self, path: &str, file_name: &str) -> String {
        let id = self.insert(&self.server.url(path), file_name, None);
        self.engine.enqueue(&id).unwrap();
        id
    }

    fn get(&self, id: &str) -> Download {
        downloads::require(&self.db.lock().unwrap(), id).unwrap()
    }

    async fn wait_for(&self, id: &str, what: &str, pred: impl Fn(&Download) -> bool) -> Download {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let d = self.get(id);
            if pred(&d) {
                return d;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}; last state: {d:?}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    async fn wait_status(&self, id: &str, status: DownloadStatus) -> Download {
        self.wait_for(id, &format!("{status:?}"), |d| d.status == status).await
    }

    async fn wait_done(&self, id: &str) -> Download {
        self.wait_for(id, "a final state", |d| {
            matches!(d.status, DownloadStatus::Completed | DownloadStatus::Failed | DownloadStatus::Cancelled)
        })
        .await
    }

    /// Waits until some (but not all) bytes have been saved.
    async fn wait_partial(&self, id: &str) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let seen = self
                .sink
                .progress
                .lock()
                .unwrap()
                .iter()
                .rev()
                .find(|p| p.id == id)
                .map(|p| p.downloaded_bytes);
            if seen.is_some_and(|n| n > 0) {
                return;
            }
            assert!(Instant::now() < deadline, "no progress for {id}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    fn temp_files(&self) -> Vec<PathBuf> {
        std::fs::read_dir(self.dir.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == TEMP_EXTENSION))
            .collect()
    }
}

fn read(path: &Option<String>) -> Vec<u8> {
    std::fs::read(Path::new(path.as_deref().expect("saved_path"))).unwrap()
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(data))
}

/// Body bytes the server actually sent (excluding the 1-byte probe). Ranges
/// in request headers would overstate it: a connection whose range was
/// split stops before the end it asked for.
fn bytes_sent(log: &[server::Logged]) -> usize {
    log.iter().filter(|l| l.range.as_deref() != Some("bytes=0-0")).map(server::Logged::sent).sum()
}

/// What a resumed download may fetch beyond what was missing: a few chunks
/// sent to a connection just before its range was split.
const SPLIT_OVERLAP: usize = 512 * 1024;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn downloads_without_range_support_as_single_stream() {
    let h = Harness::new().await;
    let data = test_data(300_000, 1);
    h.server.state.add("plain.bin", FileSpec::new(data.clone()).no_ranges());
    let id = h.add("/files/plain.bin", "plain.bin");

    let d = h.wait_done(&id).await;
    assert_eq!(d.status, DownloadStatus::Completed, "{:?}", d.error_message);
    assert!(!d.resumable);
    assert_eq!(read(&d.saved_path), data);
    assert_eq!(d.downloaded_bytes, 300_000);
    assert!(h.temp_files().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn segmented_download_reassembles_exactly() {
    let h = Harness::new().await;
    let data = test_data(4 * 1024 * 1024 + 123, 2);
    h.server.state.add("seg.bin", FileSpec::new(data.clone()).etag("v1"));
    let id = h.add("/files/seg.bin", "seg.bin");

    let d = h.wait_done(&id).await;
    assert_eq!(d.status, DownloadStatus::Completed, "{:?}", d.error_message);
    assert!(d.resumable);
    assert_eq!(read(&d.saved_path), data);

    let ranged: Vec<_> = h.server.state.requests_for("/files/seg.bin").into_iter()
        .filter(|l| l.range.as_deref().is_some_and(|r| r != "bytes=0-0")).collect();
    assert!(ranged.len() >= 8, "at least one request per planned segment, more where ranges were split");
    assert!(ranged.iter().all(|l| l.if_range.as_deref() == Some("\"v1\"")));
    assert!(segments_cleared(&h, &id));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sends_browser_sign_in_cookies_only_to_their_site() {
    let h = Harness::new().await;
    let data = test_data(2 * 1024 * 1024, 31);
    h.server.state.add("member.bin", FileSpec::new(data.clone()).etag("m1"));
    let cookie = |name: &str, domain: &str| crate::cookies::BrowserCookie {
        name: name.into(),
        value: "v".into(),
        domain: domain.into(),
        path: "/".into(),
        secure: false,
        http_only: true,
        host_only: true,
        expires: None,
    };
    let url = h.server.url("/files/member.bin");
    h.engine.cookies().remember(&url, vec![cookie("session", "127.0.0.1")]);
    h.engine.cookies().remember("https://other.example/", vec![cookie("other", "other.example")]);

    let id = h.add("/files/member.bin", "member.bin");
    let d = h.wait_done(&id).await;
    assert_eq!(d.status, DownloadStatus::Completed, "{:?}", d.error_message);
    assert_eq!(read(&d.saved_path), data);
    let log = h.server.state.requests_for("/files/member.bin");
    assert!(log.len() > 1, "probe plus segment requests");
    assert!(log.iter().all(|l| l.cookie.as_deref() == Some("session=v")), "{log:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn idle_connections_take_over_a_slow_ones_range() {
    let h = Harness::new().await;
    let total = 16 * 1024 * 1024;
    let data = test_data(total, 41);
    // 8 planned ranges of 2 MiB; the one starting at 4 MiB gets a slow connection
    // (2 MiB at 16 KiB per 40 ms is over 5 s on its own).
    let slow_at = 4 * 1024 * 1024;
    h.server.state.add("dyn.bin", FileSpec::new(data.clone()).etag("d1").slow_from(slow_at, Duration::from_millis(40)));
    let started = Instant::now();
    let id = h.add("/files/dyn.bin", "dyn.bin");

    let d = h.wait_done(&id).await;
    assert_eq!(d.status, DownloadStatus::Completed, "{:?}", d.error_message);
    assert_eq!(read(&d.saved_path), data);
    let starts: Vec<usize> = h.server.state.requests_for("/files/dyn.bin").iter()
        .filter_map(|l| l.range.as_deref()?.strip_prefix("bytes=")?.split('-').next()?.parse().ok())
        .collect();
    let planned: Vec<usize> = (0..8).map(|i| i * 2 * 1024 * 1024).collect();
    assert!(starts.iter().any(|s| !planned.contains(s)), "a range was split: {starts:?}");
    assert!(started.elapsed() < Duration::from_secs(4), "took {:?}; the slow range wasn't shared", started.elapsed());
    assert!(segments_cleared(&h, &id));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn split_ranges_survive_pause_and_restart() {
    let mut h = Harness::new().await;
    let total = 8 * 1024 * 1024;
    let data = test_data(total, 42);
    // Every connection is slow except where ranges were split, so splits are
    // in place when the download is paused.
    h.server.state.add("split.bin", FileSpec::new(data.clone()).etag("s2").delay(Duration::from_millis(15)).slow_from(0, Duration::from_millis(60)));
    let id = h.add("/files/split.bin", "split.bin");
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let segs = downloads::segments(&h.db.lock().unwrap(), &id).unwrap();
        if segs.len() > 8 {
            break;
        }
        assert!(Instant::now() < deadline, "no split happened: {segs:?}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    h.engine.pause(&id).unwrap();
    h.wait_status(&id, DownloadStatus::Paused).await;

    h.restart_engine();
    h.engine.enqueue(&id).unwrap();
    let d = h.wait_done(&id).await;
    assert_eq!(d.status, DownloadStatus::Completed, "{:?}", d.error_message);
    assert_eq!(read(&d.saved_path), data, "every byte right after resuming a split layout");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn refreshed_link_continues_where_the_old_one_stopped() {
    let h = Harness::new().await;
    let total = 4 * 1024 * 1024;
    let data = test_data(total, 51);
    h.server.state.add("expiring.bin", FileSpec::new(data.clone()).etag("e1").delay(Duration::from_millis(30)));
    let id = h.add("/files/expiring.bin", "expiring.bin");
    h.wait_partial(&id).await;
    h.engine.pause(&id).unwrap();
    let paused = h.wait_status(&id, DownloadStatus::Paused).await;

    // The same file at a fresh address (a new signed link, another mirror).
    h.server.state.add("fresh.bin", FileSpec::new(data.clone()).etag("e1"));
    h.server.state.clear_log();
    let fresh = h.server.url("/files/fresh.bin");
    let refreshed = h.engine.refresh_link(&id, &fresh).await.unwrap();
    assert_eq!(refreshed.url, fresh);
    assert_eq!(refreshed.downloaded_bytes, paused.downloaded_bytes, "progress kept");

    h.engine.enqueue(&id).unwrap();
    let d = h.wait_done(&id).await;
    assert_eq!(d.status, DownloadStatus::Completed, "{:?}", d.error_message);
    assert_eq!(read(&d.saved_path), data);
    assert!(h.server.state.requests_for("/files/expiring.bin").is_empty(), "the old link isn't used again");
    let fetched = bytes_sent(&h.server.state.requests_for("/files/fresh.bin"));
    assert!(fetched <= total - paused.downloaded_bytes as usize + SPLIT_OVERLAP, "only the rest came from the new link ({fetched})");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn refresh_refuses_a_different_file() {
    let h = Harness::new().await;
    let data = test_data(2 * 1024 * 1024, 52);
    h.server.state.add("orig.bin", FileSpec::new(data.clone()).etag("v1").delay(Duration::from_millis(30)));
    let id = h.add("/files/orig.bin", "orig.bin");
    h.wait_partial(&id).await;

    let running = h.engine.refresh_link(&id, &h.server.url("/files/orig.bin")).await.unwrap_err();
    assert!(running.to_string().contains("Pause"), "{running}");
    h.engine.pause(&id).unwrap();
    h.wait_status(&id, DownloadStatus::Paused).await;

    h.server.state.add("bigger.bin", FileSpec::new(test_data(3 * 1024 * 1024, 53)).etag("v1"));
    let size = h.engine.refresh_link(&id, &h.server.url("/files/bigger.bin")).await.unwrap_err();
    assert!(size.to_string().contains("different file"), "{size}");
    h.server.state.add("newer.bin", FileSpec::new(test_data(2 * 1024 * 1024, 54)).etag("v2"));
    let version = h.engine.refresh_link(&id, &h.server.url("/files/newer.bin")).await.unwrap_err();
    assert!(version.to_string().contains("different version"), "{version}");
    assert_eq!(h.get(&id).url, h.server.url("/files/orig.bin"), "a refused link changes nothing");
}

fn segments_cleared(h: &Harness, id: &str) -> bool {
    downloads::segments(&h.db.lock().unwrap(), id).unwrap().is_empty()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn large_file_download_is_byte_identical() {
    let h = Harness::new().await;
    let data = test_data(64 * 1024 * 1024, 3);
    let expected = sha256_hex(&data);
    h.server.state.add("large.bin", FileSpec::new(data).etag("big"));
    let id = h.insert(&h.server.url("/files/large.bin"), "large.bin", Some(&expected));
    h.engine.enqueue(&id).unwrap();

    let d = h.wait_done(&id).await;
    assert_eq!(d.status, DownloadStatus::Completed, "{:?}", d.error_message);
    assert_eq!(d.total_bytes, Some(64 * 1024 * 1024));
    assert_eq!(sha256_hex(&read(&d.saved_path)), expected);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pause_then_resume_continues_from_saved_ranges() {
    let h = Harness::new().await;
    let total = 4 * 1024 * 1024;
    let data = test_data(total, 4);
    h.server.state.add("slow.bin", FileSpec::new(data.clone()).etag("s1").delay(Duration::from_millis(30)));
    let id = h.add("/files/slow.bin", "slow.bin");

    h.wait_partial(&id).await;
    h.engine.pause(&id).unwrap();
    let paused = h.wait_status(&id, DownloadStatus::Paused).await;
    assert!(paused.downloaded_bytes > 0 && (paused.downloaded_bytes as usize) < total, "{paused:?}");
    assert_eq!(h.temp_files().len(), 1, "partial file is kept while paused");

    h.server.state.clear_log();
    h.engine.enqueue(&id).unwrap();
    let d = h.wait_done(&id).await;
    assert_eq!(d.status, DownloadStatus::Completed, "{:?}", d.error_message);
    assert_eq!(read(&d.saved_path), data);

    let second = bytes_sent(&h.server.state.requests_for("/files/slow.bin"));
    assert!(second <= total - paused.downloaded_bytes as usize + SPLIT_OVERLAP, "resume fetched {second} bytes again");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn recovers_after_app_restart() {
    let mut h = Harness::new().await;
    let total = 4 * 1024 * 1024;
    let data = test_data(total, 5);
    h.server.state.add("crash.bin", FileSpec::new(data.clone()).etag("c1").delay(Duration::from_millis(30)));
    let id = h.add("/files/crash.bin", "crash.bin");

    h.wait_partial(&id).await;
    h.engine.pause(&id).unwrap();
    let before = h.wait_status(&id, DownloadStatus::Paused).await;
    // Pretend the app died mid-transfer.
    downloads::set_status(&h.db.lock().unwrap(), &id, DownloadStatus::Downloading, None).unwrap();

    h.restart_engine();
    h.engine.start_up().unwrap();
    let recovered = h.get(&id);
    assert_eq!(recovered.status, DownloadStatus::Paused);
    assert!(recovered.error_message.unwrap().contains("Interrupted"));

    h.server.state.clear_log();
    h.engine.enqueue(&id).unwrap();
    let d = h.wait_done(&id).await;
    assert_eq!(d.status, DownloadStatus::Completed, "{:?}", d.error_message);
    assert_eq!(read(&d.saved_path), data);
    let second = bytes_sent(&h.server.state.requests_for("/files/crash.bin"));
    assert!(second <= total - before.downloaded_bytes as usize + SPLIT_OVERLAP);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn queued_downloads_restart_after_launch() {
    let mut h = Harness::new().await;
    let data = test_data(100_000, 6);
    h.server.state.add("q.bin", FileSpec::new(data.clone()));
    let id = h.insert(&h.server.url("/files/q.bin"), "q.bin", None);
    downloads::set_status(&h.db.lock().unwrap(), &id, DownloadStatus::Queued, None).unwrap();

    h.restart_engine();
    h.engine.start_up().unwrap();
    let d = h.wait_done(&id).await;
    assert_eq!(d.status, DownloadStatus::Completed);
    assert_eq!(read(&d.saved_path), data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn retries_dropped_connections_with_ranges() {
    let h = Harness::new().await;
    let data = test_data(2 * 1024 * 1024, 7);
    h.server.state.add("flaky.bin", FileSpec::new(data.clone()).etag("f").drop_after(100_000, 6));
    let id = h.add("/files/flaky.bin", "flaky.bin");

    let d = h.wait_done(&id).await;
    assert_eq!(d.status, DownloadStatus::Completed, "{:?}", d.error_message);
    assert_eq!(read(&d.saved_path), data);
    assert!(h.server.state.requests_for("/files/flaky.bin").len() > 9, "drops should have caused retries");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn retries_dropped_connection_without_ranges_from_zero() {
    let h = Harness::new().await;
    let data = test_data(500_000, 8);
    h.server.state.add("flaky1.bin", FileSpec::new(data.clone()).no_ranges().drop_after(120_000, 2));
    let id = h.add("/files/flaky1.bin", "flaky1.bin");

    let d = h.wait_done(&id).await;
    assert_eq!(d.status, DownloadStatus::Completed, "{:?}", d.error_message);
    assert_eq!(read(&d.saved_path), data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stalled_connection_times_out_and_retries() {
    let h = Harness::new().await;
    let data = test_data(600_000, 9);
    h.server.state.add("stall.bin", FileSpec::new(data.clone()).etag("st").stall(1));
    let id = h.add("/files/stall.bin", "stall.bin");

    let d = h.wait_done(&id).await;
    assert_eq!(d.status, DownloadStatus::Completed, "{:?}", d.error_message);
    assert_eq!(read(&d.saved_path), data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn changed_etag_on_resume_restarts_with_new_content() {
    let h = Harness::new().await;
    let total = 2 * 1024 * 1024;
    let spec = h.server.state.add(
        "changing.bin",
        FileSpec::new(test_data(total, 10)).etag("old").delay(Duration::from_millis(30)),
    );
    let id = h.add("/files/changing.bin", "changing.bin");
    h.wait_partial(&id).await;
    h.engine.pause(&id).unwrap();
    h.wait_status(&id, DownloadStatus::Paused).await;

    let new_data = test_data(total + 5000, 11);
    spec.replace(new_data.clone(), "new");
    h.engine.enqueue(&id).unwrap();
    let d = h.wait_done(&id).await;
    assert_eq!(d.status, DownloadStatus::Completed, "{:?}", d.error_message);
    assert_eq!(read(&d.saved_path), new_data, "must not mix old and new bytes");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn falls_back_to_single_stream_when_ranges_disappear() {
    let h = Harness::new().await;
    let total = 2 * 1024 * 1024;
    let data = test_data(total, 12);
    let spec = h.server.state.add("fallback.bin", FileSpec::new(data.clone()).etag("r").delay(Duration::from_millis(30)));
    let id = h.add("/files/fallback.bin", "fallback.bin");
    h.wait_partial(&id).await;
    h.engine.pause(&id).unwrap();
    h.wait_status(&id, DownloadStatus::Paused).await;

    spec.ranges.store(false, std::sync::atomic::Ordering::SeqCst);
    h.engine.enqueue(&id).unwrap();
    let d = h.wait_done(&id).await;
    assert_eq!(d.status, DownloadStatus::Completed, "{:?}", d.error_message);
    assert!(!d.resumable);
    assert_eq!(read(&d.saved_path), data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unknown_length_downloads_to_end_of_stream() {
    let h = Harness::new().await;
    let data = test_data(250_000, 13);
    h.server.state.add("chunked.bin", FileSpec::new(data.clone()).no_ranges().hide_length());
    let id = h.add("/files/chunked.bin", "chunked.bin");
    let d = h.wait_done(&id).await;
    assert_eq!(d.status, DownloadStatus::Completed, "{:?}", d.error_message);
    assert_eq!(read(&d.saved_path), data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn zero_byte_file_completes() {
    let h = Harness::new().await;
    h.server.state.add("empty.txt", FileSpec::new(Vec::new()));
    let id = h.add("/files/empty.txt", "empty.txt");
    let d = h.wait_done(&id).await;
    assert_eq!(d.status, DownloadStatus::Completed, "{:?}", d.error_message);
    assert_eq!(read(&d.saved_path), Vec::<u8>::new());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn http_errors_fail_with_clear_messages() {
    let h = Harness::new().await;
    let cases = [
        ("/status/404", "HTTP 404"),
        ("/status/410", "expired"),
        ("/status/403", "Access denied"),
        ("/status/401", "sign-in"),
        ("/files/missing.bin", "not found"),
    ];
    for (path, expect) in cases {
        let id = h.add(path, "x.bin");
        let d = h.wait_done(&id).await;
        assert_eq!(d.status, DownloadStatus::Failed, "{path}");
        let msg = d.error_message.unwrap();
        assert!(msg.contains(expect), "{path}: {msg}");
        assert!(!msg.contains("127.0.0.1"), "URL must not leak into messages: {msg}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn server_errors_are_retried_then_fail() {
    let h = Harness::new().await;
    let id = h.add("/status/503", "x.bin");
    let d = h.wait_done(&id).await;
    assert_eq!(d.status, DownloadStatus::Failed);
    assert!(d.error_message.unwrap().contains("503"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn follows_redirects_and_records_final_url() {
    let h = Harness::new().await;
    let data = test_data(50_000, 14);
    h.server.state.add("target.bin", FileSpec::new(data.clone()));
    let id = h.add("/redirect/target.bin", "target.bin");
    let d = h.wait_done(&id).await;
    assert_eq!(d.status, DownloadStatus::Completed, "{:?}", d.error_message);
    assert!(d.final_url.unwrap().ends_with("/files/target.bin"));
    assert_eq!(read(&d.saved_path), data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn redirect_loops_fail() {
    let h = Harness::new().await;
    let id = h.add("/loop", "loop.bin");
    let d = h.wait_done(&id).await;
    assert_eq!(d.status, DownloadStatus::Failed);
    assert!(d.error_message.unwrap().contains("redirect"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn checksum_match_and_mismatch() {
    let h = Harness::new().await;
    let data = test_data(200_000, 15);
    h.server.state.add("sum.bin", FileSpec::new(data.clone()));
    let url = h.server.url("/files/sum.bin");

    let good = h.insert(&url, "good.bin", Some(&sha256_hex(&data)));
    h.engine.enqueue(&good).unwrap();
    assert_eq!(h.wait_done(&good).await.status, DownloadStatus::Completed);

    let bad = h.insert(&url, "bad.bin", Some(&"0".repeat(64)));
    h.engine.enqueue(&bad).unwrap();
    let d = h.wait_done(&bad).await;
    assert_eq!(d.status, DownloadStatus::Failed);
    assert!(d.error_message.unwrap().contains("Checksum mismatch"));
    assert!(!h.dir.path().join("bad.bin").exists(), "a corrupt file must not be saved");
    assert!(h.temp_files().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn existing_files_are_never_overwritten() {
    let h = Harness::new().await;
    std::fs::write(h.dir.path().join("report.pdf"), b"original").unwrap();
    h.server.state.add("report.pdf", FileSpec::new(test_data(10_000, 16)));
    let id = h.add("/files/report.pdf", "report.pdf");
    let d = h.wait_done(&id).await;
    assert_eq!(d.status, DownloadStatus::Completed);
    assert!(d.saved_path.as_deref().unwrap().ends_with("report (1).pdf"));
    assert_eq!(std::fs::read(h.dir.path().join("report.pdf")).unwrap(), b"original");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancel_deletes_partial_file_and_progress() {
    let h = Harness::new().await;
    h.server.state.add("c.bin", FileSpec::new(test_data(2 * 1024 * 1024, 17)).etag("c").delay(Duration::from_millis(30)));
    let id = h.add("/files/c.bin", "c.bin");
    h.wait_partial(&id).await;
    h.engine.cancel(&id).unwrap();
    let d = h.wait_status(&id, DownloadStatus::Cancelled).await;
    assert_eq!(d.downloaded_bytes, 0);
    assert!(d.temp_path.is_none());
    assert!(h.temp_files().is_empty());

    // A cancelled download can start again from scratch.
    h.engine.enqueue(&id).unwrap();
    assert_eq!(h.wait_done(&id).await.status, DownloadStatus::Completed);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn removing_an_active_download_cleans_up() {
    let h = Harness::new().await;
    h.server.state.add("r.bin", FileSpec::new(test_data(2 * 1024 * 1024, 18)).etag("r").delay(Duration::from_millis(30)));
    let id = h.add("/files/r.bin", "r.bin");
    h.wait_partial(&id).await;
    h.engine.remove(&id).unwrap();

    let deadline = Instant::now() + Duration::from_secs(10);
    while downloads::get(&h.db.lock().unwrap(), &id).unwrap().is_some() {
        assert!(Instant::now() < deadline, "row not removed");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(h.temp_files().is_empty());
    assert_eq!(h.sink.removed.lock().unwrap().as_slice(), [id]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn respects_concurrency_limit() {
    let h = Harness::new().await;
    h.engine.set_limits(2, 2).unwrap();
    let mut ids = Vec::new();
    for i in 0..5 {
        let name = format!("c{i}.bin");
        h.server.state.add(&name, FileSpec::new(test_data(400_000, 20 + i)).etag("x").delay(Duration::from_millis(5)));
        ids.push(h.add(&format!("/files/{name}"), &name));
    }
    let mut max_seen = 0;
    loop {
        let all: Vec<Download> = ids.iter().map(|id| h.get(id)).collect();
        let running = all.iter().filter(|d| d.status == DownloadStatus::Downloading).count();
        max_seen = max_seen.max(running);
        if all.iter().all(|d| d.status == DownloadStatus::Completed) {
            break;
        }
        assert!(all.iter().all(|d| d.status != DownloadStatus::Failed), "{all:?}");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(max_seen <= 2, "saw {max_seen} running at once");
    assert_eq!(max_seen, 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reports_progress_speed_and_eta() {
    let h = Harness::new().await;
    h.server.state.add("p.bin", FileSpec::new(test_data(2 * 1024 * 1024, 30)).etag("p").delay(Duration::from_millis(20)));
    let id = h.add("/files/p.bin", "p.bin");
    assert_eq!(h.wait_done(&id).await.status, DownloadStatus::Completed);

    let events: Vec<Progress> = h.sink.progress.lock().unwrap().iter().filter(|p| p.id == id).cloned().collect();
    assert!(events.len() >= 3, "expected several progress events, got {}", events.len());
    assert!(events.windows(2).all(|w| w[0].downloaded_bytes <= w[1].downloaded_bytes));
    assert!(events.iter().all(|p| p.total_bytes == Some(2 * 1024 * 1024)));
    assert_eq!(events.last().unwrap().downloaded_bytes, 2 * 1024 * 1024, "final event shows completion");
    assert!(events.iter().any(|p| p.speed > 0.0 && p.eta_seconds.is_some()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn survives_many_drops_on_a_throttled_server() {
    let h = Harness::new().await;
    let data = test_data(10 * 1024 * 1024, 40);
    h.server.state.add(
        "flaky2.bin",
        FileSpec::new(data.clone()).etag("f2").drop_after(300_000, 20).delay(Duration::from_millis(5)),
    );
    let id = h.add("/files/flaky2.bin", "flaky2.bin");
    let d = h.wait_done(&id).await;
    assert_eq!(d.status, DownloadStatus::Completed, "{:?}", d.error_message);
    assert_eq!(read(&d.saved_path), data);
}

// ------------------------------------------------------------- M5: queues

impl Harness {
    fn insert_in(&self, path: &str, file_name: &str, queue_id: &str, priority: i32, speed_limit: Option<i64>) -> String {
        let conn = self.db.lock().unwrap();
        downloads::insert(
            &conn,
            &downloads::InsertDownload {
                url: &self.server.url(path),
                file_name,
                destination_dir: &self.dir.path().to_string_lossy(),
                expected_sha256: None,
                kind: crate::models::DownloadKind::File,
                title: None,
                media_options: None,
                referrer: None,
                queue_id,
                priority,
                category: None,
                speed_limit,
            },
        )
        .unwrap()
        .id
    }

    fn queue_running(&self, id: &str) -> bool {
        queues::require(&self.db.lock().unwrap(), id).unwrap().running
    }

    /// Ids in the order they first started downloading.
    fn start_order(&self) -> Vec<String> {
        let mut seen = Vec::new();
        for d in self.sink.updates.lock().unwrap().iter().filter(|d| d.status == DownloadStatus::Downloading) {
            if !seen.contains(&d.id) {
                seen.push(d.id.clone());
            }
        }
        seen
    }
}

fn local(h: u32, m: u32) -> chrono::NaiveDateTime {
    chrono::NaiveDate::from_ymd_opt(2026, 10, 19).unwrap().and_hms_opt(h, m, 0).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn starts_by_priority_then_queue_order() {
    let h = Harness::new().await;
    h.engine.set_limits(1, 1).unwrap();
    h.engine.stop_queue("main").unwrap();
    for name in ["low.bin", "normal.bin", "high.bin", "normal2.bin"] {
        h.server.state.add(name, FileSpec::new(test_data(50_000, 50)));
    }
    let low = h.insert_in("/files/low.bin", "low.bin", "main", -1, None);
    let normal = h.insert_in("/files/normal.bin", "normal.bin", "main", 0, None);
    let high = h.insert_in("/files/high.bin", "high.bin", "main", 1, None);
    let normal2 = h.insert_in("/files/normal2.bin", "normal2.bin", "main", 0, None);
    for id in [&low, &normal, &high, &normal2] {
        h.engine.enqueue(id).unwrap();
    }
    assert!(h.start_order().is_empty(), "a stopped queue starts nothing");
    assert_eq!(h.get(&high).status, DownloadStatus::Queued);

    h.engine.start_queue("main").unwrap();
    for id in [&low, &normal, &high, &normal2] {
        assert_eq!(h.wait_done(id).await.status, DownloadStatus::Completed);
    }
    assert_eq!(h.start_order(), [high, normal, normal2, low]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stopping_a_queue_requeues_running_downloads_with_progress() {
    let h = Harness::new().await;
    let night = queues::save(&h.db.lock().unwrap(), "Night", &QueueSchedule::default(), None).unwrap();
    let total = 2 * 1024 * 1024;
    let data = test_data(total, 51);
    h.server.state.add("night.bin", FileSpec::new(data.clone()).etag("n").delay(Duration::from_millis(30)));
    let id = h.insert_in("/files/night.bin", "night.bin", &night.id, 0, None);
    h.engine.enqueue(&id).unwrap();
    h.wait_partial(&id).await;

    h.engine.stop_queue(&night.id).unwrap();
    let waiting = h.wait_status(&id, DownloadStatus::Queued).await;
    assert!(waiting.downloaded_bytes > 0, "progress is kept");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(h.get(&id).status, DownloadStatus::Queued, "stays put while the queue is stopped");

    h.engine.start_queue(&night.id).unwrap();
    let d = h.wait_done(&id).await;
    assert_eq!(d.status, DownloadStatus::Completed, "{:?}", d.error_message);
    assert_eq!(read(&d.saved_path), data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn schedule_starts_and_stops_a_queue() {
    let h = Harness::new().await;
    let every_day = QueueSchedule { enabled: true, start: "01:00".into(), stop: Some("02:00".into()), days: (0..7).collect() };
    let q = queues::save(&h.db.lock().unwrap(), "Overnight", &every_day, None).unwrap();
    queues::set_running(&h.db.lock().unwrap(), &q.id, false).unwrap();
    h.server.state.add("s.bin", FileSpec::new(test_data(2 * 1024 * 1024, 52)).etag("s").delay(Duration::from_millis(30)));
    let id = h.insert_in("/files/s.bin", "s.bin", &q.id, 0, None);
    h.engine.enqueue(&id).unwrap();

    h.engine.tick_at(local(0, 50), 0).unwrap();
    assert!(!h.queue_running(&q.id));
    h.engine.tick_at(local(0, 59), 0).unwrap();
    assert_eq!(h.get(&id).status, DownloadStatus::Queued);

    h.engine.tick_at(local(1, 0), 0).unwrap();
    assert!(h.queue_running(&q.id), "window opened");
    h.wait_partial(&id).await;

    h.engine.tick_at(local(2, 0), 0).unwrap();
    assert!(!h.queue_running(&q.id), "window closed");
    let d = h.wait_status(&id, DownloadStatus::Queued).await;
    assert!(d.downloaded_bytes > 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn launch_inside_a_window_starts_the_queue() {
    let h = Harness::new().await;
    let s = QueueSchedule { enabled: true, start: "01:00".into(), stop: Some("05:00".into()), days: (0..7).collect() };
    let q = queues::save(&h.db.lock().unwrap(), "Overnight", &s, None).unwrap();
    queues::set_running(&h.db.lock().unwrap(), &q.id, false).unwrap();
    h.server.state.add("w.bin", FileSpec::new(test_data(20_000, 53)));
    let id = h.insert_in("/files/w.bin", "w.bin", &q.id, 0, None);
    downloads::set_status(&h.db.lock().unwrap(), &id, DownloadStatus::Queued, None).unwrap();

    h.engine.tick_at(local(3, 0), 0).unwrap();
    assert_eq!(h.wait_done(&id).await.status, DownloadStatus::Completed);
    // Ran dry: a scheduled queue waits for its next window.
    let deadline = Instant::now() + Duration::from_secs(5);
    while h.queue_running(&q.id) {
        assert!(Instant::now() < deadline, "queue should stop once empty");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn failed_downloads_retry_automatically_then_give_up() {
    let h = Harness::new().await;
    h.engine.set_auto_retry(1, Duration::ZERO);
    let id = h.add("/status/503", "x.bin");
    let d = h.wait_for(&id, "a planned retry", |d| d.status == DownloadStatus::Failed && d.retry_at.is_some()).await;
    assert_eq!(d.auto_retries, 1);
    assert!(h.sink.failed.lock().unwrap().is_empty(), "no failure notice while a retry is planned");

    h.engine.tick_at(local(12, 0), d.retry_at.unwrap()).unwrap();
    let d = h.wait_for(&id, "the final failure", |d| d.status == DownloadStatus::Failed && d.retry_at.is_none() && d.updated_at > 0).await;
    let deadline = Instant::now() + Duration::from_secs(10);
    while h.sink.failed.lock().unwrap().is_empty() {
        assert!(Instant::now() < deadline, "expected one failure notice");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(d.auto_retries, 1);
    assert_eq!(h.sink.failed.lock().unwrap().as_slice(), std::slice::from_ref(&id));

    // Broken links aren't retried.
    let gone = h.add("/status/404", "gone.bin");
    let d = h.wait_done(&gone).await;
    assert!(d.retry_at.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn per_download_speed_limit_is_respected() {
    let h = Harness::new().await;
    let data = test_data(600 * 1024, 54);
    h.server.state.add("limited.bin", FileSpec::new(data.clone()).etag("l"));
    let id = h.insert_in("/files/limited.bin", "limited.bin", "main", 0, Some(400 * 1024));
    let started = Instant::now();
    h.engine.enqueue(&id).unwrap();
    let d = h.wait_done(&id).await;
    let secs = started.elapsed().as_secs_f64();
    assert_eq!(d.status, DownloadStatus::Completed, "{:?}", d.error_message);
    assert_eq!(read(&d.saved_path), data);
    assert!(secs >= 1.2, "600 KB at 400 KB/s took only {secs:.2}s");

    // Lifting the limit mid-download takes effect at once.
    h.server.state.add("lifted.bin", FileSpec::new(test_data(4 * 1024 * 1024, 55)).etag("x"));
    let id = h.insert_in("/files/lifted.bin", "lifted.bin", "main", 0, Some(100 * 1024));
    let started = Instant::now();
    h.engine.enqueue(&id).unwrap();
    h.wait_partial(&id).await;
    h.engine.set_speed_limit(&id, None).unwrap();
    assert_eq!(h.wait_done(&id).await.status, DownloadStatus::Completed);
    assert!(started.elapsed() < Duration::from_secs(15), "4 MB at 100 KB/s would take 40 s");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reports_all_done_once_when_the_list_empties() {
    let h = Harness::new().await;
    for i in 0..3 {
        h.server.state.add(&format!("d{i}.bin"), FileSpec::new(test_data(80_000, 60 + i)));
    }
    let ids: Vec<String> = (0..3).map(|i| h.add(&format!("/files/d{i}.bin"), &format!("d{i}.bin"))).collect();
    for id in &ids {
        assert_eq!(h.wait_done(id).await.status, DownloadStatus::Completed);
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while h.sink.all_done.load(std::sync::atomic::Ordering::SeqCst) == 0 {
        assert!(Instant::now() < deadline, "all_done not reported");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(h.sink.all_done.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(h.sink.completed.lock().unwrap().len(), 3);
}

// --------------------------------------------------------- M6: network guard

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn local_network_links_are_blocked_unless_allowed() {
    let h = Harness::new().await;
    h.engine.set_auto_retry(2, Duration::ZERO);
    h.engine.net_guard().set_allow_local(false);
    h.server.state.add("lan.bin", FileSpec::new(test_data(10_000, 70)));

    // A literal IP and a name that resolves to one are both refused, without retries.
    for url in [h.server.url("/files/lan.bin"), h.server.url("/files/lan.bin").replace("127.0.0.1", "localhost")] {
        let id = h.insert(&url, "lan.bin", None);
        h.engine.enqueue(&id).unwrap();
        let d = h.wait_done(&id).await;
        assert_eq!(d.status, DownloadStatus::Failed, "{url}");
        assert!(d.error_message.as_deref().unwrap_or("").contains("local network"), "{:?}", d.error_message);
        assert!(d.retry_at.is_none(), "blocked links are not retried");
    }
    assert!(h.server.state.requests_for("/files/lan.bin").is_empty(), "nothing reached the server");

    h.engine.net_guard().set_allow_local(true);
    let id = h.add("/files/lan.bin", "lan.bin");
    assert_eq!(h.wait_done(&id).await.status, DownloadStatus::Completed);
}

#[test]
fn redirects_may_not_downgrade_or_reach_reserved_addresses() {
    use super::http::redirect_refusal;
    let guard = crate::net_guard::NetGuard::new(true);
    let u = |s: &str| url::Url::parse(s).unwrap();
    assert!(redirect_refusal(&[u("https://a.example/x")], &u("http://b.example/x"), &guard).is_some());
    assert!(redirect_refusal(&[u("http://a.example/x")], &u("https://b.example/x"), &guard).is_none());
    assert!(redirect_refusal(&[u("https://a.example/x")], &u("http://169.254.169.254/latest/meta-data"), &guard).is_some());
    assert!(redirect_refusal(&[u("http://a.example/x")], &u("ftp://b.example/x"), &guard).is_some());
    guard.set_allow_local(false);
    assert!(redirect_refusal(&[u("http://a.example/x")], &u("http://192.168.1.1/admin"), &guard).is_some());
}

#[test]
fn referrer_follows_the_browser_default_policy() {
    use super::http::referrer_for;
    let page = Some("https://site.com/watch?v=1&token=secret#t=10");
    assert_eq!(referrer_for(page, "https://site.com/file.zip").as_deref(), Some("https://site.com/watch?v=1&token=secret"));
    assert_eq!(referrer_for(page, "https://cdn.example/file.zip").as_deref(), Some("https://site.com/"));
    assert_eq!(referrer_for(page, "http://site.com/file.zip"), None, "never from HTTPS to HTTP");
    assert_eq!(referrer_for(None, "https://site.com/file.zip"), None);
}

#[test]
fn yt_dlp_errors_never_show_signed_links() {
    let msg = crate::media::output::friendly_error(&["ERROR: [generic] Unable to fetch https://cdn.example/v.mp4?sig=abc123&expires=9 because of a server error".to_string()]);
    assert!(!msg.contains("sig=abc123") && !msg.contains("https://"), "{msg}");
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn completed_downloads_carry_the_mark_of_the_web() {
    let h = Harness::new().await;
    h.server.state.add("motw.exe", FileSpec::new(test_data(20_000, 80)));
    let id = h.add("/files/motw.exe", "motw.exe");
    let d = h.wait_done(&id).await;
    assert_eq!(d.status, DownloadStatus::Completed);
    let mut stream = std::path::PathBuf::from(d.saved_path.unwrap()).into_os_string();
    stream.push(":Zone.Identifier");
    let mark = std::fs::read_to_string(std::path::PathBuf::from(stream)).expect("Zone.Identifier stream");
    assert!(mark.contains("ZoneId=3") && mark.contains("HostUrl=http://127.0.0.1"), "{mark}");
}
