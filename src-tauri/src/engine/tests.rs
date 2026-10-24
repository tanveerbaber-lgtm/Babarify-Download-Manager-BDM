//! Engine integration tests against a local HTTP server.


use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::test_server::{self as server, FileSpec, TestServer, test_data};

use super::events::{EventSink, Progress};
use super::files::TEMP_EXTENSION;
use super::{Engine, EngineConfig};
use crate::db::{self, Database, downloads};
use crate::models::{Download, DownloadStatus};

#[derive(Default)]
struct RecordingSink {
    progress: Mutex<Vec<Progress>>,
    updates: Mutex<Vec<Download>>,
    removed: Mutex<Vec<String>>,
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
}

fn test_config() -> EngineConfig {
    EngineConfig {
        connect_timeout: Duration::from_secs(2),
        read_timeout: Duration::from_millis(800),
        max_retries: 5,
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

/// Total bytes requested by ranged GETs (excluding the 1-byte probe).
fn bytes_requested(log: &[server::Logged], total: usize) -> usize {
    log.iter()
        .filter_map(|l| l.range.as_deref())
        .filter(|r| *r != "bytes=0-0")
        .map(|r| {
            let spec = r.trim_start_matches("bytes=");
            let (s, e) = spec.split_once('-').unwrap();
            let s: usize = s.parse().unwrap();
            let e: usize = if e.is_empty() { total - 1 } else { e.parse().unwrap() };
            e - s + 1
        })
        .sum()
}

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
    assert_eq!(ranged.len(), 8, "expected one request per segment");
    assert!(ranged.iter().all(|l| l.if_range.as_deref() == Some("\"v1\"")));
    assert!(segments_cleared(&h, &id));
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

    let second = bytes_requested(&h.server.state.requests_for("/files/slow.bin"), total);
    assert!(second <= total - paused.downloaded_bytes as usize, "resume re-requested {second} bytes");
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
    let second = bytes_requested(&h.server.state.requests_for("/files/crash.bin"), total);
    assert!(second <= total - before.downloaded_bytes as usize);
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
