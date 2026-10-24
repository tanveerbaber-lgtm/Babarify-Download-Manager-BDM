//! Local IPC between the native host and the running app.
//!
//! The app listens on a per-user named pipe that refuses remote clients.
//! Each request carries a random token that the app writes to the user's
//! AppData at startup, so other users on the machine can't drive it.

use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::protocol::{self, BrowserRequest, BrowserResponse, PipeRequest};

pub const APP_IDENTIFIER: &str = "com.babarify.bdm";
const TOKEN_FILE: &str = "ipc-token";
const APP_PATH_FILE: &str = "app-path";

/// `%APPDATA%\com.babarify.bdm` — the same folder Tauri uses for app data.
pub fn data_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join(APP_IDENTIFIER))
}

/// Pipe names are machine-wide, so include the user name.
pub fn pipe_name() -> String {
    let user: String = std::env::var("USERNAME")
        .unwrap_or_else(|_| "user".into())
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    format!(r"\\.\pipe\{APP_IDENTIFIER}.{user}")
}

/// Creates a fresh token and records where the app is, for the host.
pub fn write_session_files(data_dir: &Path) -> io::Result<String> {
    std::fs::create_dir_all(data_dir)?;
    let token = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
    std::fs::write(data_dir.join(TOKEN_FILE), &token)?;
    if let Ok(exe) = std::env::current_exe() {
        std::fs::write(data_dir.join(APP_PATH_FILE), exe.to_string_lossy().as_bytes())?;
    }
    Ok(token)
}

pub fn read_token(data_dir: &Path) -> Option<String> {
    std::fs::read_to_string(data_dir.join(TOKEN_FILE)).ok().map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}

pub fn read_app_path(data_dir: &Path) -> Option<PathBuf> {
    let path = PathBuf::from(std::fs::read_to_string(data_dir.join(APP_PATH_FILE)).ok()?.trim());
    path.is_file().then_some(path)
}

// ------------------------------------------------------------------- server

/// Answers one request; async because listing formats runs yt-dlp.
pub type Handler = Arc<dyn Fn(BrowserRequest) -> Pin<Box<dyn Future<Output = BrowserResponse> + Send>> + Send + Sync>;

/// Serves pipe requests until the process exits.
#[cfg(windows)]
pub async fn serve(name: String, token: String, handler: Handler) -> io::Result<()> {
    use tokio::net::windows::named_pipe::ServerOptions;

    // `first_pipe_instance` fails if another process already owns the name,
    // so nothing else can sit in front of BDM and collect requests.
    let mut server = ServerOptions::new().first_pipe_instance(true).reject_remote_clients(true).create(&name)?;
    loop {
        server.connect().await?;
        let connected = server;
        server = ServerOptions::new().reject_remote_clients(true).create(&name)?;
        let (token, handler) = (token.clone(), handler.clone());
        tokio::spawn(async move {
            let _ = handle_connection(connected, &token, &handler).await;
        });
    }
}

#[cfg(windows)]
async fn handle_connection(
    mut pipe: tokio::net::windows::named_pipe::NamedPipeServer,
    token: &str,
    handler: &Handler,
) -> io::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let read = async {
        let mut len = [0u8; 4];
        pipe.read_exact(&mut len).await?;
        let len = u32::from_le_bytes(len) as usize;
        if len > protocol::MAX_MESSAGE {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "message too large"));
        }
        let mut buf = vec![0u8; len];
        pipe.read_exact(&mut buf).await?;
        Ok::<_, io::Error>(buf)
    };
    let buf = tokio::time::timeout(Duration::from_secs(5), read)
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "client too slow"))??;

    let response = match serde_json::from_slice::<PipeRequest>(&buf) {
        Ok(req) if protocol::constant_time_eq(&req.token, token) => match req.request.validate() {
            Ok(()) => handler(req.request).await,
            Err(e) => BrowserResponse::error(e.to_string()),
        },
        Ok(_) => BrowserResponse::error("Not authorized."),
        Err(_) => BrowserResponse::error("Malformed request."),
    };
    let mut out = Vec::new();
    protocol::write_json(&mut out, &response)?;
    pipe.write_all(&out).await?;
    pipe.flush().await?;
    Ok(())
}

// ------------------------------------------------------------------- client

/// Sends one request to the running app. Fails fast if no app is listening.
pub fn send(name: &str, token: &str, request: &BrowserRequest) -> io::Result<BrowserResponse> {
    let mut pipe = open_pipe(name, Duration::from_secs(2))?;
    protocol::write_json(&mut pipe, &PipeRequest { token: token.to_string(), request: request.clone() })?;
    let frame = protocol::read_frame(&mut pipe)?.ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "app closed the pipe"))?;
    serde_json::from_slice(&frame).map_err(io::Error::other)
}

/// Opens the pipe, waiting briefly while all instances are busy.
fn open_pipe(name: &str, busy_wait: Duration) -> io::Result<std::fs::File> {
    const ERROR_PIPE_BUSY: i32 = 231;
    let deadline = Instant::now() + busy_wait;
    loop {
        match std::fs::OpenOptions::new().read(true).write(true).open(name) {
            Ok(f) => return Ok(f),
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e),
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    fn test_pipe() -> String {
        format!(r"\\.\pipe\bdm-test-{}", uuid::Uuid::new_v4().simple())
    }

    async fn start(name: &str, token: &str) {
        let handler: Handler = Arc::new(|req| {
            Box::pin(async move {
                match req {
                    BrowserRequest::Ping => BrowserResponse::running("9.9.9"),
                    BrowserRequest::Download { url, .. } => BrowserResponse { error: Some(url), ..BrowserResponse::ok() },
                    _ => BrowserResponse::ok(),
                }
            })
        });
        let (n, t) = (name.to_string(), token.to_string());
        tokio::spawn(async move { serve(n, t, handler).await });
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    fn send_blocking(name: &str, token: &str, req: BrowserRequest) -> io::Result<BrowserResponse> {
        let (n, t) = (name.to_string(), token.to_string());
        std::thread::spawn(move || send(&n, &t, &req)).join().unwrap()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn serves_authorized_requests() {
        let name = test_pipe();
        start(&name, "secret").await;
        let r = send_blocking(&name, "secret", BrowserRequest::Ping).unwrap();
        assert_eq!(r, BrowserResponse::running("9.9.9"));
        // Several requests in a row each get their own connection.
        for _ in 0..3 {
            let r = send_blocking(&name, "secret", BrowserRequest::Download { url: "https://e.com/x".into(), file_name: None, referrer: None, mime: None, size: None }).unwrap();
            assert_eq!(r.error.as_deref(), Some("https://e.com/x"));
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn rejects_wrong_tokens_and_invalid_requests() {
        let name = test_pipe();
        start(&name, "secret").await;
        let r = send_blocking(&name, "guess", BrowserRequest::Ping).unwrap();
        assert!(!r.ok && r.error.as_deref() == Some("Not authorized."));
        let r = send_blocking(&name, "secret", BrowserRequest::Media { url: "file:///etc/passwd".into(), page_title: None }).unwrap();
        assert!(!r.ok);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn refuses_a_second_server_on_the_same_name() {
        let name = test_pipe();
        start(&name, "a").await;
        let handler: Handler = Arc::new(|_| Box::pin(async { BrowserResponse::ok() }));
        let second = tokio::time::timeout(Duration::from_secs(2), serve(name, "b".into(), handler)).await.unwrap();
        assert!(second.is_err(), "pipe squatting must fail");
    }

    #[test]
    fn missing_app_fails_fast() {
        let started = Instant::now();
        assert!(send(&test_pipe(), "t", &BrowserRequest::Ping).is_err());
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn session_files_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let token = write_session_files(dir.path()).unwrap();
        assert_eq!(token.len(), 64);
        assert_eq!(read_token(dir.path()).as_deref(), Some(token.as_str()));
        assert!(read_app_path(dir.path()).is_some(), "current test binary path recorded");
        assert_ne!(write_session_files(dir.path()).unwrap(), token, "fresh token each launch");
    }
}
