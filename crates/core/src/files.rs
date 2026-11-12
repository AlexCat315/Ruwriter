//! Document I/O. All targets use a temporary file plus a rename; Windows needs
//! an explicit removal first because `rename` will not replace an existing file.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub fn read_document(path: &Path) -> io::Result<String> {
    let bytes = fs::read(path)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub fn write_document(path: &Path, text: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = temporary_path(path);
    let result = fs::write(&temporary, text.as_bytes()).and_then(|()| replace(&temporary, path));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn temporary_path(path: &Path) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "document".to_owned());
    path.with_file_name(format!(".{name}.{}-{sequence:x}.tmp", std::process::id()))
}

#[cfg(target_os = "windows")]
fn replace(temporary: &Path, destination: &Path) -> io::Result<()> {
    match fs::rename(temporary, destination) {
        Ok(()) => Ok(()),
        Err(_) if destination.exists() => {
            fs::remove_file(destination)?;
            fs::rename(temporary, destination)
        }
        Err(error) => Err(error),
    }
}

#[cfg(not(target_os = "windows"))]
fn replace(temporary: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(temporary, destination)
}

/// What a file looked like the last time Ruwriter read or wrote it. A
/// different stamp means someone else changed it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Stamp {
    /// Modification time in nanoseconds since the epoch.
    pub modified: u64,
    pub size: u64,
}

impl Stamp {
    pub fn modified_seconds(self) -> u64 {
        self.modified / 1_000_000_000
    }
}

pub fn stamp(path: &Path) -> Option<Stamp> {
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX))
        .unwrap_or_default();
    Some(Stamp {
        modified,
        size: metadata.len(),
    })
}

/// `dir/stem.extension`, or `dir/stem 2.extension`, `stem 3`… when taken.
/// `ignore` is a path that may be reused (the file being renamed).
pub fn unique_path(dir: &Path, stem: &str, extension: &str, ignore: Option<&Path>) -> PathBuf {
    let name = |index: usize| {
        let stem = if index < 2 {
            stem.to_owned()
        } else {
            format!("{stem} {index}")
        };
        if extension.is_empty() {
            dir.join(stem)
        } else {
            dir.join(format!("{stem}.{extension}"))
        }
    };
    let mut index = 1;
    loop {
        let candidate = name(index);
        if ignore.is_some_and(|ignore| same_path(ignore, &candidate))
            || fs::symlink_metadata(&candidate).is_err()
        {
            return candidate;
        }
        index += 1;
    }
}

/// Case-insensitive file systems (macOS, Windows) treat `A.md` and `a.md` as
/// one file; a rename between them is a rename to itself.
fn same_path(left: &Path, right: &Path) -> bool {
    left == right
        || left
            .to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
}

/// Moves a file or folder, creating the destination's parent. Falls back to
/// copy-and-delete across volumes.
pub fn move_path(from: &Path, to: &Path) -> io::Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(error) if from.is_dir() => Err(error),
        Err(_) => {
            fs::copy(from, to)?;
            fs::remove_file(from)
        }
    }
}
