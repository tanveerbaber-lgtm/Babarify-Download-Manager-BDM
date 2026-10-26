//! Free disk space: checked before a download starts and watched while one
//! of unknown size runs, so BDM pauses with a clear message instead of
//! failing halfway when the drive fills up.

use std::path::Path;
use std::sync::Arc;

/// Space left free for Windows and other programs.
pub const DEFAULT_RESERVE: u64 = 256 * 1024 * 1024;

/// Measures the free bytes on a path's drive.
pub type Measure = dyn Fn(&Path) -> Option<u64> + Send + Sync;

/// How free space is measured; tests swap in their own.
#[derive(Clone)]
pub struct FreeSpace(pub Arc<Measure>);

impl Default for FreeSpace {
    fn default() -> Self {
        Self(Arc::new(free_space))
    }
}

impl std::fmt::Debug for FreeSpace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FreeSpace")
    }
}

impl FreeSpace {
    pub fn of(&self, path: &Path) -> Option<u64> {
        (self.0)(path)
    }
}

/// Bytes available to this user on the drive holding `path` (or its nearest
/// existing parent, as a new folder may not exist yet).
pub fn free_space(path: &Path) -> Option<u64> {
    let existing = path.ancestors().find(|p| p.is_dir())?;
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetDiskFreeSpaceExW(dir: *const u16, free_to_caller: *mut u64, total: *mut u64, free_total: *mut u64) -> i32;
        }
        let wide: Vec<u16> = existing.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let mut free = 0u64;
        // SAFETY: `wide` is NUL-terminated and outlives the call; the
        // optional out-pointers may be null.
        let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut()) };
        (ok != 0).then_some(free)
    }
    #[cfg(not(windows))]
    {
        let _ = existing;
        None
    }
}

/// "E:" for E:\Downloads, else the folder itself.
fn drive_of(path: &Path) -> String {
    match path.components().next() {
        Some(std::path::Component::Prefix(p)) => p.as_os_str().to_string_lossy().into_owned(),
        _ => path.display().to_string(),
    }
}

/// Why `need` more bytes won't fit on `dir`'s drive (keeping `reserve`
/// free), if they won't. Unknown free space never blocks a download.
pub fn shortfall(dir: &Path, need: u64, reserve: u64, free: &FreeSpace) -> Option<String> {
    let available = free.of(dir)?;
    (available < need.saturating_add(reserve)).then(|| {
        let size = crate::validation::human_size;
        format!(
            "Not enough space on {}: this download needs {} more and {} is free. Free up space, then resume it.",
            drive_of(dir),
            size(need),
            size(available)
        )
    })
}

/// The message for a download paused because the drive is nearly full.
pub fn nearly_full(dir: &Path, free: &FreeSpace) -> String {
    let left = free.of(dir).map(crate::validation::human_size).unwrap_or_else(|| "little".into());
    format!("Paused: the drive {} is almost full ({left} free). Free up space, then resume it.", drive_of(dir))
}

/// Whether an I/O error means the disk is full.
pub fn is_disk_full(e: &std::io::Error) -> bool {
    // ERROR_HANDLE_DISK_FULL (39) and ERROR_DISK_FULL (112) on Windows.
    e.kind() == std::io::ErrorKind::StorageFull || matches!(e.raw_os_error(), Some(39 | 112))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed(n: u64) -> FreeSpace {
        FreeSpace(Arc::new(move |_| Some(n)))
    }

    #[test]
    fn shortfall_keeps_a_reserve() {
        let mb = 1024 * 1024;
        let dir = Path::new(r"E:\Downloads");
        assert!(shortfall(dir, 100 * mb, 50 * mb, &fixed(200 * mb)).is_none());
        let msg = shortfall(dir, 100 * mb, 50 * mb, &fixed(120 * mb)).unwrap();
        assert!(msg.starts_with("Not enough space on E:"), "{msg}");
        assert!(msg.contains("100 MB more") && msg.contains("120 MB is free"), "{msg}");
        assert!(shortfall(dir, u64::MAX, 50 * mb, &FreeSpace(Arc::new(|_| None))).is_none(), "unknown never blocks");
    }

    #[test]
    fn reads_real_free_space() {
        let here = std::env::current_dir().unwrap();
        assert!(free_space(&here).is_some_and(|n| n > 0));
        assert!(free_space(&here.join("not-created-yet").join("deeper")).is_some(), "nearest existing parent");
    }

    #[test]
    fn recognises_disk_full_errors() {
        assert!(is_disk_full(&std::io::Error::from_raw_os_error(112)));
        assert!(!is_disk_full(&std::io::Error::from_raw_os_error(5)));
    }
}
