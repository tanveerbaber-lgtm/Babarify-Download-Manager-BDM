//! Temporary files, safe finalization and checksums.

use std::fs::OpenOptions;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::error::EngineResult;

pub const TEMP_EXTENSION: &str = "bdmpart";
const MAX_NAME_ATTEMPTS: u32 = 9_999;

/// The partial file lives next to the destination so the final rename stays
/// on one volume (and is therefore atomic).
pub fn temp_path_for(dest_dir: &Path, file_name: &str, id: &str) -> PathBuf {
    let short_id: String = id.chars().filter(|c| c.is_ascii_hexdigit()).take(8).collect();
    dest_dir.join(format!("{file_name}.{short_id}.{TEMP_EXTENSION}"))
}

/// `name.ext` → `name (n).ext`
fn numbered_name(file_name: &str, n: u32) -> String {
    match file_name.rfind('.') {
        Some(i) if i > 0 => format!("{} ({n}){}", &file_name[..i], &file_name[i..]),
        _ => format!("{file_name} ({n})"),
    }
}

/// Moves the finished temp file to a name in `dest_dir` that doesn't exist
/// yet, never overwriting another file.
///
/// The target name is reserved by creating it with `create_new` (which fails
/// if anything already has that name), then the temp file replaces that empty
/// placeholder in one rename.
pub fn finalize(temp: &Path, dest_dir: &Path, file_name: &str) -> EngineResult<PathBuf> {
    for n in 0..=MAX_NAME_ATTEMPTS {
        let name = if n == 0 { file_name.to_string() } else { numbered_name(file_name, n) };
        let target = dest_dir.join(&name);
        match OpenOptions::new().write(true).create_new(true).open(&target) {
            Ok(placeholder) => {
                drop(placeholder);
                if let Err(e) = std::fs::rename(temp, &target) {
                    let _ = std::fs::remove_file(&target);
                    return Err(e.into());
                }
                return Ok(target);
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(std::io::Error::new(ErrorKind::AlreadyExists, "no free file name in the destination folder").into())
}

/// Lowercase hex SHA-256 of a file (blocking; call from `spawn_blocking`).
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Removes a partial file (or a media temp folder), ignoring "not found".
/// Only BDM's own temp names are ever deleted, whatever the path says.
pub fn remove_if_exists(path: &Path) -> std::io::Result<()> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let is_dir = std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir());
    let ours = if is_dir { name.starts_with(".bdm-media-") } else { name.ends_with(&format!(".{TEMP_EXTENSION}")) };
    if !ours {
        return match std::fs::symlink_metadata(path) {
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            _ => Err(std::io::Error::other(format!("refusing to delete {name}: not a BDM temp file"))),
        };
    }
    if is_dir {
        return std::fs::remove_dir_all(path);
    }
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_bdm_temp_files_are_ever_removed() {
        let dir = tempfile::tempdir().unwrap();
        let keep = dir.path().join("report.pdf");
        std::fs::write(&keep, b"mine").unwrap();
        assert!(remove_if_exists(&keep).is_err());
        assert!(keep.exists(), "a normal file must survive a bad temp path");
        let folder = dir.path().join("Photos");
        std::fs::create_dir(&folder).unwrap();
        assert!(remove_if_exists(&folder).is_err());
        assert!(folder.exists());

        let part = temp_path_for(dir.path(), "a.zip", "0123456789abcdef");
        std::fs::write(&part, b"x").unwrap();
        remove_if_exists(&part).unwrap();
        assert!(!part.exists());
        let media = dir.path().join(".bdm-media-01234567");
        std::fs::create_dir(&media).unwrap();
        remove_if_exists(&media).unwrap();
        assert!(!media.exists());
        remove_if_exists(&dir.path().join("gone.bin")).unwrap();
    }

    #[test]
    fn numbered_names_keep_extension() {
        assert_eq!(numbered_name("movie.mp4", 1), "movie (1).mp4");
        assert_eq!(numbered_name("archive.tar.gz", 2), "archive.tar (2).gz");
        assert_eq!(numbered_name("README", 3), "README (3)");
        assert_eq!(numbered_name(".hidden", 1), ".hidden (1)");
    }

    #[test]
    fn finalize_never_overwrites_existing_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"keep me").unwrap();
        std::fs::write(dir.path().join("a (1).txt"), b"keep me too").unwrap();
        let temp = dir.path().join("a.txt.1234.bdmpart");
        std::fs::write(&temp, b"new").unwrap();

        let saved = finalize(&temp, dir.path(), "a.txt").unwrap();
        assert_eq!(saved.file_name().unwrap(), "a (2).txt");
        assert_eq!(std::fs::read(&saved).unwrap(), b"new");
        assert_eq!(std::fs::read(dir.path().join("a.txt")).unwrap(), b"keep me");
        assert!(!temp.exists());
    }

    #[test]
    fn temp_path_is_in_destination() {
        let p = temp_path_for(Path::new("C:\\dl"), "f.zip", "1f2e3d4c-aaaa");
        assert_eq!(p, PathBuf::from("C:\\dl\\f.zip.1f2e3d4c.bdmpart"));
    }

    #[test]
    fn sha256_of_known_content() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x");
        std::fs::write(&p, b"abc").unwrap();
        assert_eq!(
            sha256_file(&p).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
