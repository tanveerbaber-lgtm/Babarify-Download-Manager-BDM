//! Scanning finished downloads with Microsoft Defender's command-line scanner.
//!
//! Defender's real-time protection already checks files as they're written;
//! the explicit scan gives BDM a result to show. Defender quarantines or
//! removes what it finds, as it would for any other file. When Defender
//! isn't available (another antivirus replaced it, or it's turned off) the
//! scan is skipped.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::media::process;

const SCAN_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Stored in `downloads.scan_result`.
pub const SCANNING: &str = "scanning";
pub const CLEAN: &str = "clean";
pub const THREAT: &str = "threat";
pub const UNAVAILABLE: &str = "unavailable";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanOutcome {
    Clean,
    Threat,
    /// The scanner couldn't run or didn't give a result (why, for the log).
    Unavailable(String),
}

impl ScanOutcome {
    pub fn stored(&self) -> &'static str {
        match self {
            Self::Clean => CLEAN,
            Self::Threat => THREAT,
            Self::Unavailable(_) => UNAVAILABLE,
        }
    }
}

/// MpCmdRun's exit codes: 0 no threat, 2 threat found; anything else means
/// the scan didn't complete (Defender off, file gone, another error).
pub fn outcome_for(code: Option<i32>) -> ScanOutcome {
    match code {
        Some(0) => ScanOutcome::Clean,
        Some(2) => ScanOutcome::Threat,
        Some(c) => ScanOutcome::Unavailable(format!("scanner exit code {c:#x}")),
        None => ScanOutcome::Unavailable("scanner was stopped".into()),
    }
}

/// Defender's active platform (the "CurrentVersion" junction), else the
/// newest platform folder, else the copy in Program Files.
pub fn defender_path() -> Option<PathBuf> {
    let platform = std::env::var_os("ProgramData").map(|p| PathBuf::from(p).join(r"Microsoft\Windows Defender\Platform"));
    if let Some(current) = platform.as_ref().map(|dir| dir.join(r"CurrentVersion\MpCmdRun.exe"))
        && current.is_file()
    {
        return Some(current);
    }
    let newest = platform.and_then(|dir| {
        let mut versions: Vec<PathBuf> = std::fs::read_dir(dir)
            .ok()?
            .flatten()
            .map(|e| e.path().join("MpCmdRun.exe"))
            .filter(|p| p.is_file())
            .collect();
        // Folder names are versions like "4.18.25090.3009-0"; compare them numerically.
        versions.sort_by_key(|p| version_key(p));
        versions.pop()
    });
    newest.or_else(|| {
        let classic = PathBuf::from(std::env::var_os("ProgramFiles")?).join(r"Windows Defender\MpCmdRun.exe");
        classic.is_file().then_some(classic)
    })
}

fn version_key(exe: &Path) -> Vec<u64> {
    let name = exe.parent().and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    name.split(['.', '-']).map(|part| part.parse().unwrap_or(0)).collect()
}

/// Scans one file. Defender handles anything it finds (quarantine/removal).
pub async fn scan(scanner: &Path, file: &Path) -> ScanOutcome {
    let mut cmd = process::command(scanner);
    cmd.args(["-Scan", "-ScanType", "3", "-File"]).arg(file);
    match tokio::time::timeout(SCAN_TIMEOUT, cmd.output()).await {
        Err(_) => ScanOutcome::Unavailable("scan took too long".into()),
        Ok(Err(e)) => ScanOutcome::Unavailable(format!("couldn't run the scanner: {e}")),
        Ok(Ok(out)) => outcome_for(out.status.code()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_defender_exit_codes() {
        assert_eq!(outcome_for(Some(0)), ScanOutcome::Clean);
        assert_eq!(outcome_for(Some(2)), ScanOutcome::Threat);
        assert_eq!(outcome_for(Some(0x8050_8023_u32 as i32)).stored(), UNAVAILABLE);
        assert_eq!(outcome_for(None).stored(), UNAVAILABLE);
    }

    #[test]
    fn newest_platform_version_wins() {
        let p = |v: &str| PathBuf::from(format!(r"C:\ProgramData\Microsoft\Windows Defender\Platform\{v}\MpCmdRun.exe"));
        let mut versions = [p("4.18.25090.3009-0"), p("4.18.3.1"), p("4.18.24110.7-0")];
        versions.sort_by_key(|x| version_key(x));
        assert_eq!(versions.last(), Some(&p("4.18.25090.3009-0")));
    }
}
