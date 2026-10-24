//! The native-messaging host the browser starts (`bdm-native-host.exe`).
//!
//! It checks which extension started it, reads framed requests from stdin,
//! forwards them to the running app over the pipe (starting BDM first if
//! needed) and writes the replies to stdout. It never acts on a request
//! itself, so its whole job is validation and relaying.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::EXTENSION_IDS;
use super::ipc;
use super::protocol::{self, BrowserRequest, BrowserResponse};

const APP_START_TIMEOUT: Duration = Duration::from_secs(25);

/// Where the host finds the app.
pub struct HostEnv {
    pub pipe: String,
    pub data_dir: PathBuf,
    /// Starts BDM; returns false if it couldn't.
    pub launch: Box<dyn Fn(&Path) -> bool>,
    pub start_timeout: Duration,
}

impl HostEnv {
    pub fn from_system() -> Option<Self> {
        Some(Self {
            pipe: ipc::pipe_name(),
            data_dir: ipc::data_dir()?,
            launch: Box::new(launch_app),
            start_timeout: APP_START_TIMEOUT,
        })
    }
}

/// Chrome and Edge pass the caller's origin as the first argument.
pub fn caller_allowed(args: &[String]) -> bool {
    args.iter()
        .find_map(|a| a.strip_prefix("chrome-extension://"))
        .map(|rest| rest.trim_end_matches('/'))
        .is_some_and(|id| EXTENSION_IDS.contains(&id))
}

/// Runs until the browser closes stdin. Returns the process exit code.
pub fn run(args: &[String], input: &mut impl Read, output: &mut impl Write, env: &HostEnv) -> i32 {
    if !caller_allowed(args) {
        let _ = protocol::write_json(output, &BrowserResponse::error("This extension isn't allowed to use BDM."));
        return 1;
    }
    loop {
        let frame = match protocol::read_frame(input) {
            Ok(Some(f)) => f,
            Ok(None) => return 0,
            Err(_) => {
                let _ = protocol::write_json(output, &BrowserResponse::error("Malformed message."));
                return 1;
            }
        };
        let response = match serde_json::from_slice::<BrowserRequest>(&frame) {
            Ok(req) => match req.validate() {
                Ok(()) => handle(req, env),
                Err(e) => BrowserResponse::error(e.to_string()),
            },
            Err(_) => BrowserResponse::error("Unsupported message."),
        };
        if protocol::write_json(output, &response).is_err() {
            return 1;
        }
    }
}

fn handle(req: BrowserRequest, env: &HostEnv) -> BrowserResponse {
    if let Some(resp) = try_send(&req, env) {
        return resp;
    }
    // A ping only reports state; it never starts BDM.
    if req == BrowserRequest::Ping {
        return BrowserResponse::stopped();
    }
    let Some(app) = ipc::read_app_path(&env.data_dir) else {
        return BrowserResponse::error("BDM isn't running and couldn't be found. Start Babarify Download Manager once.");
    };
    if !(env.launch)(&app) {
        return BrowserResponse::error("Couldn't start BDM.");
    }
    let deadline = Instant::now() + env.start_timeout;
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(250));
        if let Some(resp) = try_send(&req, env) {
            return resp;
        }
    }
    BrowserResponse::error("BDM didn't start in time. Try again.")
}

/// Forwards to the app; `None` if it isn't listening.
fn try_send(req: &BrowserRequest, env: &HostEnv) -> Option<BrowserResponse> {
    let token = ipc::read_token(&env.data_dir)?;
    ipc::send(&env.pipe, &token, req).ok()
}

/// Starts BDM detached so it keeps running after the browser closes the host.
fn launch_app(app: &Path) -> bool {
    let mut cmd = std::process::Command::new(app);
    cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        // Leave the browser's job object if allowed, so closing the browser
        // doesn't close BDM; fall back if the job forbids breaking away.
        cmd.creation_flags(DETACHED_PROCESS | CREATE_BREAKAWAY_FROM_JOB);
        if cmd.spawn().is_ok() {
            return true;
        }
        cmd.creation_flags(DETACHED_PROCESS);
    }
    cmd.spawn().is_ok()
}

#[cfg(all(test, windows))]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use super::*;
    use crate::browser::ipc::Handler;

    const ORIGIN: &str = "chrome-extension://emaohblojdddihemfkbcchbfgaaanjkb/";

    fn frames(reqs: &[&str]) -> Vec<u8> {
        let mut buf = Vec::new();
        for r in reqs {
            protocol::write_frame(&mut buf, r.as_bytes()).unwrap();
        }
        buf
    }

    fn responses(mut out: &[u8]) -> Vec<BrowserResponse> {
        let mut v = Vec::new();
        while let Some(f) = protocol::read_frame(&mut out).unwrap() {
            v.push(serde_json::from_slice(&f).unwrap());
        }
        v
    }

    fn env(dir: &Path, pipe: &str, launch: Box<dyn Fn(&Path) -> bool>) -> HostEnv {
        HostEnv { pipe: pipe.into(), data_dir: dir.into(), launch, start_timeout: Duration::from_secs(5) }
    }

    #[test]
    fn rejects_unknown_extensions() {
        assert!(caller_allowed(&[ORIGIN.into(), "--parent-window=0".into()]));
        assert!(!caller_allowed(&["chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/".into()]));
        assert!(!caller_allowed(&[]));
        let dir = tempfile::tempdir().unwrap();
        let mut out = Vec::new();
        let code = run(&["chrome-extension://evil/".into()], &mut frames(&[r#"{"type":"ping"}"#]).as_slice(), &mut out, &env(dir.path(), "x", Box::new(|_| true)));
        assert_eq!(code, 1);
        assert!(!responses(&out)[0].ok);
    }

    #[test]
    fn ping_reports_stopped_without_starting_the_app() {
        let dir = tempfile::tempdir().unwrap();
        let launched = Arc::new(AtomicBool::new(false));
        let l = launched.clone();
        let mut out = Vec::new();
        let input = frames(&[r#"{"type":"ping"}"#, r#"{"type":"bogus"}"#, r#"{"type":"download","url":"ftp://x"}"#]);
        let code = run(&[ORIGIN.into()], &mut input.as_slice(), &mut out, &env(dir.path(), r"\\.\pipe\bdm-none", Box::new(move |_| { l.store(true, Ordering::SeqCst); true })));
        assert_eq!(code, 0);
        let r = responses(&out);
        assert_eq!(r[0], BrowserResponse::stopped());
        assert_eq!(r[1].error.as_deref(), Some("Unsupported message."));
        assert!(!r[2].ok, "invalid URL rejected by the host itself");
        assert!(!launched.load(Ordering::SeqCst));
    }

    #[test]
    fn starts_the_app_and_forwards_when_it_is_not_running() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let pipe = format!(r"\\.\pipe\bdm-host-test-{}", uuid::Uuid::new_v4().simple());
        // `write_session_files` records the test binary as "the app".
        let token = ipc::write_session_files(dir.path()).unwrap();

        // "Launching" starts the pipe server, like the real app would.
        let (p, t, handle) = (pipe.clone(), token.clone(), rt.handle().clone());
        let launch: Box<dyn Fn(&Path) -> bool> = Box::new(move |_| {
            let handler: Handler = Arc::new(|req| match req {
                BrowserRequest::Download { file_name, .. } => BrowserResponse { error: file_name, ..BrowserResponse::ok() },
                _ => BrowserResponse::running("1.0"),
            });
            handle.spawn(ipc::serve(p.clone(), t.clone(), handler));
            true
        });
        let mut out = Vec::new();
        let input = frames(&[r#"{"type":"download","url":"https://e.com/a.zip","fileName":"a.zip"}"#, r#"{"type":"ping"}"#]);
        let code = run(&[ORIGIN.into()], &mut input.as_slice(), &mut out, &env(dir.path(), &pipe, launch));
        assert_eq!(code, 0);
        let r = responses(&out);
        assert!(r[0].ok && r[0].error.as_deref() == Some("a.zip"), "{:?}", r[0]);
        assert_eq!(r[1], BrowserResponse::running("1.0"));
    }
}
