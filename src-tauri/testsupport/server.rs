//! Local HTTP server for engine tests and manual checks.
//!
//! Serves in-memory files with configurable Range support, ETags,
//! throttling, dropped connections and stalls, plus redirect and error
//! endpoints. Used by the engine's unit tests and by `examples/test_server.rs`.

#![allow(dead_code)]

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;

const CHUNK: usize = 16 * 1024;

pub struct FileSpec {
    pub data: Mutex<Arc<Vec<u8>>>,
    pub etag: Mutex<Option<String>>,
    pub ranges: AtomicBool,
    /// Omit Content-Length on full responses (chunked transfer).
    pub hide_length: bool,
    /// Abort the body after this many bytes, for the next `drop_times` responses.
    pub drop_after: Option<usize>,
    pub drop_times: AtomicU32,
    /// Send one chunk then go silent, for the next `stall_times` responses.
    pub stall_times: AtomicU32,
    /// Delay before each 16 KiB chunk.
    pub chunk_delay: Duration,
}

impl FileSpec {
    pub fn new(data: Vec<u8>) -> Self {
        Self {
            data: Mutex::new(Arc::new(data)),
            etag: Mutex::new(None),
            ranges: AtomicBool::new(true),
            hide_length: false,
            drop_after: None,
            drop_times: AtomicU32::new(0),
            stall_times: AtomicU32::new(0),
            chunk_delay: Duration::ZERO,
        }
    }
    pub fn no_ranges(self) -> Self {
        self.ranges.store(false, Ordering::SeqCst);
        self
    }
    pub fn etag(self, etag: &str) -> Self {
        *self.etag.lock().unwrap() = Some(format!("\"{etag}\""));
        self
    }
    pub fn hide_length(mut self) -> Self {
        self.hide_length = true;
        self
    }
    pub fn drop_after(mut self, bytes: usize, times: u32) -> Self {
        self.drop_after = Some(bytes);
        self.drop_times = AtomicU32::new(times);
        self
    }
    pub fn stall(self, times: u32) -> Self {
        self.stall_times.store(times, Ordering::SeqCst);
        self
    }
    pub fn delay(mut self, per_chunk: Duration) -> Self {
        self.chunk_delay = per_chunk;
        self
    }
    /// Replaces the content and ETag, as if the file changed on the server.
    pub fn replace(&self, data: Vec<u8>, etag: &str) {
        *self.data.lock().unwrap() = Arc::new(data);
        *self.etag.lock().unwrap() = Some(format!("\"{etag}\""));
    }
}

#[derive(Debug, Clone)]
pub struct Logged {
    pub path: String,
    pub range: Option<String>,
    pub if_range: Option<String>,
    pub cookie: Option<String>,
}

#[derive(Default)]
pub struct ServerState {
    pub files: Mutex<HashMap<String, Arc<FileSpec>>>,
    pub log: Mutex<Vec<Logged>>,
    /// Canned JSON served at `/api/<path>` (e.g. a fake GitHub release).
    pub json: Mutex<HashMap<String, String>>,
}

impl ServerState {
    pub fn add(&self, name: &str, spec: FileSpec) -> Arc<FileSpec> {
        let spec = Arc::new(spec);
        self.files.lock().unwrap().insert(name.to_string(), spec.clone());
        spec
    }

    pub fn requests_for(&self, path: &str) -> Vec<Logged> {
        self.log.lock().unwrap().iter().filter(|l| l.path == path).cloned().collect()
    }

    pub fn set_json(&self, path: &str, body: String) {
        self.json.lock().unwrap().insert(path.trim_start_matches('/').to_string(), body);
    }

    pub fn clear_log(&self) {
        self.log.lock().unwrap().clear();
    }
}

pub struct TestServer {
    pub addr: SocketAddr,
    pub state: Arc<ServerState>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl TestServer {
    pub async fn start() -> Self {
        Self::start_on("127.0.0.1:0").await
    }

    pub async fn start_on(addr: &str) -> Self {
        let state = Arc::new(ServerState::default());
        let app = Router::new()
            .route("/files/{name}", get(serve_file))
            .route("/redirect/{name}", get(redirect))
            .route("/guarded/{name}", get(guarded_file))
            .route("/tokened/{name}", get(tokened_file))
            .route("/status/{code}", get(status))
            .route("/api/{*path}", get(api_json))
            .route("/page/{name}", get(html_page))
            .route("/loop", get(|| async { (StatusCode::FOUND, [(header::LOCATION, "/loop")]) }))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind(addr).await.expect("bind test server");
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = rx.await;
                })
                .await
                .ok();
        });
        Self { addr, state, shutdown: Some(tx) }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

/// Deterministic pseudo-random bytes.
pub fn test_data(len: usize, seed: u64) -> Vec<u8> {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x >> 24) as u8
        })
        .collect()
}

fn parse_range(value: &str) -> Option<(usize, Option<usize>)> {
    let spec = value.trim().strip_prefix("bytes=")?;
    let (start, end) = spec.split_once('-')?;
    let start = start.parse().ok()?;
    let end = if end.is_empty() { None } else { Some(end.parse().ok()?) };
    Some((start, end))
}

fn take_one(counter: &AtomicU32) -> bool {
    let mut n = counter.load(Ordering::SeqCst);
    while n > 0 {
        match counter.compare_exchange(n, n - 1, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return true,
            Err(current) => n = current,
        }
    }
    false
}

async fn serve_file(
    State(state): State<Arc<ServerState>>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Response {
    let get_header = |h: header::HeaderName| headers.get(h).and_then(|v| v.to_str().ok()).map(str::to_owned);
    let range_header = get_header(header::RANGE);
    let if_range = get_header(header::IF_RANGE);
    state.log.lock().unwrap().push(Logged {
        path: format!("/files/{name}"),
        range: range_header.clone(),
        if_range: if_range.clone(),
        cookie: get_header(header::COOKIE),
    });

    let Some(spec) = state.files.lock().unwrap().get(&name).cloned() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let data = spec.data.lock().unwrap().clone();
    let etag = spec.etag.lock().unwrap().clone();
    let ranges = spec.ranges.load(Ordering::SeqCst);
    let total = data.len();

    let mut status = StatusCode::OK;
    let (mut start, mut end) = (0usize, total);
    if ranges && let Some((s, e)) = range_header.as_deref().and_then(parse_range) {
        let validator_ok = if_range.is_none() || if_range == etag;
        if validator_ok {
            if s >= total {
                let mut resp = StatusCode::RANGE_NOT_SATISFIABLE.into_response();
                resp.headers_mut()
                    .insert(header::CONTENT_RANGE, HeaderValue::from_str(&format!("bytes */{total}")).unwrap());
                return resp;
            }
            start = s;
            end = e.map(|e| (e + 1).min(total)).unwrap_or(total);
            status = StatusCode::PARTIAL_CONTENT;
        }
    }
    let body_len = end - start;

    let drop_at = spec.drop_after.filter(|&n| body_len > n && take_one(&spec.drop_times));
    let stall = body_len > CHUNK && take_one(&spec.stall_times);
    let delay = spec.chunk_delay;

    let stream = async_stream(data, start, end, drop_at, stall, delay);
    let mut resp = Response::builder().status(status);
    let h = resp.headers_mut().unwrap();
    // Like a real download server: browsers save these instead of showing them.
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/octet-stream"));
    if ranges {
        h.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    }
    if let Some(etag) = &etag {
        h.insert(header::ETAG, HeaderValue::from_str(etag).unwrap());
    }
    if status == StatusCode::PARTIAL_CONTENT {
        h.insert(
            header::CONTENT_RANGE,
            HeaderValue::from_str(&format!("bytes {}-{}/{}", start, end - 1, total)).unwrap(),
        );
    }
    if !(spec.hide_length && status == StatusCode::OK) {
        h.insert(header::CONTENT_LENGTH, HeaderValue::from(body_len));
    }
    resp.body(Body::from_stream(stream)).unwrap()
}

fn async_stream(
    data: Arc<Vec<u8>>,
    start: usize,
    end: usize,
    drop_at: Option<usize>,
    stall: bool,
    delay: Duration,
) -> impl futures_util::Stream<Item = Result<Bytes, std::io::Error>> {
    futures_util::stream::unfold((start, 0usize, false), move |(pos, sent, done)| {
        let data = data.clone();
        async move {
            if done || pos >= end {
                return None;
            }
            if let Some(limit) = drop_at
                && sent >= limit
            {
                return Some((Err(std::io::Error::other("simulated drop")), (pos, sent, true)));
            }
            if stall && sent >= CHUNK {
                tokio::time::sleep(Duration::from_secs(3600)).await;
            }
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
            let mut n = CHUNK.min(end - pos);
            if let Some(limit) = drop_at {
                n = n.min(limit - sent).max(1);
            }
            let chunk = Bytes::copy_from_slice(&data[pos..pos + n]);
            Some((Ok(chunk), (pos + n, sent + n, false)))
        }
    })
}

async fn api_json(State(state): State<Arc<ServerState>>, Path(path): Path<String>) -> Response {
    match state.json.lock().unwrap().get(&path).cloned() {
        Some(body) => ([(header::CONTENT_TYPE, "application/json")], body).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// A web page with no media on it (for "unsupported site" tests).
async fn html_page(Path(name): Path<String>) -> impl IntoResponse {
    let body = format!("<!doctype html><html><head><title>{name}</title></head><body><p>No video here.</p></body></html>");
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], body)
}

/// Serves a file only to requests with a `Referer`, like CDNs that only play
/// their streams on the site's own pages.
async fn guarded_file(State(state): State<Arc<ServerState>>, Path(name): Path<String>, headers: HeaderMap) -> Response {
    if headers.get(header::REFERER).is_none() {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(spec) = state.files.lock().unwrap().get(&name).cloned() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let data = spec.data.lock().unwrap().clone();
    let kind = if name.ends_with(".m3u8") { "application/vnd.apple.mpegurl" } else { "video/mp2t" };
    ([(header::CONTENT_TYPE, kind)], data.to_vec()).into_response()
}

/// Serves a file only with the header a page's player script adds.
async fn tokened_file(state: State<Arc<ServerState>>, Path(name): Path<String>, headers: HeaderMap) -> Response {
    if headers.get("x-stream-token").and_then(|v| v.to_str().ok()) != Some("ok") {
        return StatusCode::FORBIDDEN.into_response();
    }
    let mut with_referer = headers.clone();
    with_referer.insert(header::REFERER, HeaderValue::from_static("http://page.test/"));
    guarded_file(state, Path(name), with_referer).await
}

async fn redirect(Path(name): Path<String>) -> impl IntoResponse {
    (StatusCode::FOUND, [(header::LOCATION, format!("/files/{name}"))])
}

async fn status(Path(code): Path<u16>) -> impl IntoResponse {
    StatusCode::from_u16(code).unwrap_or(StatusCode::BAD_REQUEST)
}
