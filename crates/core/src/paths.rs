//! Per-platform storage roots. Every supported Cove host has an explicit root,
//! including the mobile targets that are not exercised by desktop tests.

use std::path::{Path, PathBuf};

/// Override for tests, packaging and managed deployments.
const OVERRIDE: &str = "RUWRITER_DATA_DIR";

/// Directory holding `library.json` and the `documents/` folder.
pub fn data_root() -> PathBuf {
    if let Some(path) = std::env::var_os(OVERRIDE) {
        return PathBuf::from(path);
    }
    platform_root().join("Ruwriter")
}

/// Where documents owned by the library are stored.
pub fn documents_directory(root: &Path) -> PathBuf {
    root.join("documents")
}

/// The library index, relative to the data root.
pub fn library_index_path(root: &Path) -> PathBuf {
    root.join("library.json")
}

#[cfg(target_os = "windows")]
fn home_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(not(target_os = "windows"))]
fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(target_os = "macos")]
fn platform_root() -> PathBuf {
    home_dir().join("Documents")
}

#[cfg(target_os = "windows")]
fn platform_root() -> PathBuf {
    home_dir().join("Documents")
}

#[cfg(target_os = "linux")]
fn platform_root() -> PathBuf {
    std::env::var_os("XDG_DOCUMENTS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join("Documents"))
}

#[cfg(target_os = "ios")]
fn platform_root() -> PathBuf {
    // UIKit applications own `HOME/Documents`; the Cove host publishes HOME.
    home_dir().join("Documents")
}

#[cfg(target_os = "android")]
fn platform_root() -> PathBuf {
    // `cove-hosts` injects the Activity's `filesDir`; the packaged resource
    // bundle sits next to it and is the fallback before HOME.
    std::env::var_os("COVE_APP_STORAGE_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("PACKAGE_RESOURCE_BUNDLE_PATH").map(|value| {
                let path = PathBuf::from(value);
                path.parent().map(Path::to_path_buf).unwrap_or(path)
            })
        })
        .unwrap_or_else(home_dir)
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "windows",
    target_os = "linux",
    target_os = "ios",
    target_os = "android"
)))]
fn platform_root() -> PathBuf {
    home_dir()
}
