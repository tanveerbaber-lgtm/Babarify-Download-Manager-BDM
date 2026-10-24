//! Registering the native-messaging host with Chrome and Edge.
//!
//! Per-user registration (HKCU) needs no admin rights: a manifest JSON
//! names the host executable and the only extension IDs allowed to start it,
//! and a registry key per browser points at that manifest.

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{EXTENSION_IDS, HOST_EXE, HOST_NAME};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Browser {
    Chrome,
    Edge,
}

impl Browser {
    pub const ALL: [Browser; 2] = [Browser::Chrome, Browser::Edge];

    fn key_path(self) -> String {
        let vendor = match self {
            Self::Chrome => r"Software\Google\Chrome",
            Self::Edge => r"Software\Microsoft\Edge",
        };
        format!(r"{vendor}\NativeMessagingHosts\{HOST_NAME}")
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Chrome => "Google Chrome",
            Self::Edge => "Microsoft Edge",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserStatus {
    pub browser: Browser,
    pub name: &'static str,
    /// The browser's registry key points at our manifest.
    pub registered: bool,
    /// The manifest names a host executable that exists.
    pub host_found: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HostManifest {
    pub name: String,
    pub description: String,
    pub path: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub allowed_origins: Vec<String>,
}

pub fn manifest_for(host_exe: &Path) -> HostManifest {
    HostManifest {
        name: HOST_NAME.to_string(),
        description: "Babarify Download Manager".to_string(),
        path: host_exe.to_string_lossy().into_owned(),
        kind: "stdio".to_string(),
        allowed_origins: EXTENSION_IDS.iter().map(|id| format!("chrome-extension://{id}/")).collect(),
    }
}

/// The host executable shipped next to the app.
pub fn host_exe_path() -> AppResult<PathBuf> {
    let exe = std::env::current_exe().map_err(|e| AppError::Io(e.to_string()))?;
    Ok(exe.with_file_name(HOST_EXE))
}

pub fn manifest_path(data_dir: &Path) -> PathBuf {
    data_dir.join("native-host").join(format!("{HOST_NAME}.json"))
}

/// Writes the manifest and points both browsers at it.
pub fn register(data_dir: &Path, host_exe: &Path) -> AppResult<()> {
    if !host_exe.is_file() {
        return Err(AppError::Validation(format!(
            "The browser helper ({HOST_EXE}) is missing next to BDM. Reinstall BDM."
        )));
    }
    let manifest = manifest_path(data_dir);
    std::fs::create_dir_all(manifest.parent().expect("has parent"))?;
    std::fs::write(&manifest, serde_json::to_vec_pretty(&manifest_for(host_exe))?)?;
    for browser in Browser::ALL {
        set_key(browser, Some(&manifest))?;
    }
    Ok(())
}

/// Removes both registry keys and the manifest.
pub fn unregister(data_dir: &Path) -> AppResult<()> {
    for browser in Browser::ALL {
        set_key(browser, None)?;
    }
    let _ = std::fs::remove_file(manifest_path(data_dir));
    Ok(())
}

pub fn status(data_dir: &Path) -> Vec<BrowserStatus> {
    let manifest = manifest_path(data_dir);
    let host_found = std::fs::read(&manifest)
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|v| v["path"].as_str().map(PathBuf::from))
        .is_some_and(|p| p.is_file());
    Browser::ALL
        .into_iter()
        .map(|browser| BrowserStatus {
            browser,
            name: browser.label(),
            registered: read_key(browser).is_some_and(|p| same_path(&p, &manifest)),
            host_found,
        })
        .collect()
}

fn same_path(a: &Path, b: &Path) -> bool {
    a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy())
}

#[cfg(windows)]
fn set_key(browser: Browser, manifest: Option<&Path>) -> AppResult<()> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    match manifest {
        Some(path) => {
            let (key, _) = hkcu.create_subkey(browser.key_path()).map_err(|e| AppError::Io(e.to_string()))?;
            key.set_value("", &path.to_string_lossy().into_owned()).map_err(|e| AppError::Io(e.to_string()))?;
        }
        None => match hkcu.delete_subkey_all(browser.key_path()) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(AppError::Io(e.to_string())),
            _ => {}
        },
    }
    Ok(())
}

#[cfg(windows)]
fn read_key(browser: Browser) -> Option<PathBuf> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey(browser.key_path()).ok()?;
    key.get_value::<String, _>("").ok().map(PathBuf::from)
}

#[cfg(not(windows))]
fn set_key(_: Browser, _: Option<&Path>) -> AppResult<()> {
    Err(AppError::Validation("Browser integration is only available on Windows.".into()))
}

#[cfg(not(windows))]
fn read_key(_: Browser) -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_allows_only_our_extensions() {
        let m = manifest_for(Path::new(r"C:\Program Files\BDM\bdm-native-host.exe"));
        let json = serde_json::to_value(&m).unwrap();
        assert_eq!(json["name"], "com.babarify.bdm");
        assert_eq!(json["type"], "stdio");
        assert_eq!(json["path"], r"C:\Program Files\BDM\bdm-native-host.exe");
        let origins: Vec<&str> = json["allowed_origins"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert!(!origins.is_empty());
        assert!(origins.iter().all(|o| o.starts_with("chrome-extension://") && o.ends_with('/') && !o.contains('*')));
    }

    #[test]
    fn registration_requires_the_host_executable() {
        let dir = tempfile::tempdir().unwrap();
        let err = register(dir.path(), &dir.path().join("missing.exe")).unwrap_err();
        assert!(err.to_string().contains("missing"));
    }
}
