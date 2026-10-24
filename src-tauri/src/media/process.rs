//! Spawning helper processes (yt-dlp, ffmpeg, deno) without console windows.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A command with no console window, null stdin and piped output. Arguments
/// are passed as a list — never through a shell.
pub fn command(program: &Path) -> Command {
    let mut cmd = Command::new(program);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

/// Kills a process and everything it started (yt-dlp runs ffmpeg as a child).
pub async fn kill_tree(pid: u32) {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("taskkill");
        cmd.args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW);
        let _ = tokio::time::timeout(Duration::from_secs(10), cmd.status()).await;
    }
    #[cfg(not(windows))]
    {
        let _ = Command::new("kill").args(["-9", &pid.to_string()]).status().await;
    }
}

/// Runs a program to completion with a timeout and returns its stdout.
pub async fn output(program: &Path, args: &[&str], timeout: Duration) -> Option<String> {
    let mut cmd = command(program);
    cmd.args(args);
    let out = tokio::time::timeout(timeout, cmd.output()).await.ok()?.ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}
