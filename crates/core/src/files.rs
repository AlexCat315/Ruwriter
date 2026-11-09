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
