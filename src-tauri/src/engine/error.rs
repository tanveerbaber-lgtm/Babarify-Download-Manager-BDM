//! Errors raised while transferring a file, with retry classification and
//! user-facing messages.

use std::time::Duration;

use crate::error::AppError;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// The server answered with an unexpected HTTP status.
    #[error("HTTP {status}")]
    Http {
        status: u16,
        retry_after: Option<Duration>,
    },
    /// Connection, TLS, timeout or premature end of stream.
    #[error("network error: {message}")]
    Network { message: String, retryable: bool },
    /// The remote file is no longer the one we started downloading.
    #[error("remote file changed")]
    Changed,
    /// Local file-system failure.
    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error("checksum mismatch")]
    Checksum { expected: String, actual: String },
    #[error("size mismatch: expected {expected}, got {actual}")]
    SizeMismatch { expected: u64, actual: u64 },
    #[error("{0}")]
    App(#[from] AppError),
    /// yt-dlp failed; the message is already user-facing.
    #[error("{0}")]
    Media(String),
    /// Stopped by pause, cancel or remove.
    #[error("stopped")]
    Stopped,
}

impl EngineError {
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Http { status, .. } => matches!(status, 408 | 425 | 429 | 500..=599),
            Self::Network { retryable, .. } => *retryable,
            _ => false,
        }
    }

    /// Server-requested delay before retrying (from `Retry-After`).
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::Http { retry_after, .. } => *retry_after,
            _ => None,
        }
    }

    /// Message shown to the user. Never includes the URL, which may carry tokens.
    pub fn user_message(&self) -> String {
        match self {
            Self::Http { status, .. } => match status {
                401 | 407 => format!(
                    "The server requires a sign-in (HTTP {status}). Downloads that need authentication aren't supported yet."
                ),
                403 => "Access denied by the server (HTTP 403). The link may have expired or need a sign-in.".into(),
                404 => "File not found on the server (HTTP 404). The link may be broken or expired.".into(),
                410 => "The link has expired (HTTP 410). Get a fresh link and try again.".into(),
                416 => "The server rejected the requested byte range (HTTP 416).".into(),
                429 => "The server is limiting requests (HTTP 429). Try again later.".into(),
                500..=599 => format!("The server had a problem (HTTP {status}). Try again later."),
                _ => format!("The server responded with HTTP {status}."),
            },
            Self::Network { message, .. } => format!("Network problem: {message}"),
            Self::Changed => "The file on the server changed during the download.".into(),
            Self::Io(e) => format!("Couldn't write the file: {e}"),
            Self::Checksum { expected, actual } => format!(
                "Checksum mismatch: expected SHA-256 {expected}, got {actual}. The file was not saved."
            ),
            Self::SizeMismatch { expected, actual } => format!(
                "The download is incomplete: expected {expected} bytes but received {actual}."
            ),
            Self::App(e) => e.to_string(),
            Self::Media(m) => m.clone(),
            Self::Stopped => "Stopped.".into(),
        }
    }

    pub fn from_reqwest(err: reqwest::Error) -> Self {
        let retryable = !err.is_redirect()
            && (err.is_timeout() || err.is_connect() || err.is_request() || err.is_body() || err.is_decode());
        let message = if err.is_timeout() {
            "the connection timed out".to_string()
        } else if err.is_connect() {
            "couldn't connect to the server".to_string()
        } else if err.is_redirect() {
            "too many redirects or an unsupported redirect".to_string()
        } else if err.is_body() || err.is_decode() {
            "the connection was interrupted".to_string()
        } else {
            // Strip the URL so signed query strings never reach the UI or logs.
            err.without_url().to_string()
        };
        Self::Network { message, retryable }
    }

    pub fn premature_end() -> Self {
        Self::Network {
            message: "the server closed the connection early".into(),
            retryable: true,
        }
    }
}

pub type EngineResult<T> = Result<T, EngineError>;
