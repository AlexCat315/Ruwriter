//! Version snapshots: plain text copies of one document, kept under
//! `.ruwriter/snapshots/` so they travel with the library and can be deleted
//! or rebuilt at any time. Snapshots never touch the document itself.

use crate::files::write_document;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// How many snapshots one document keeps. Older ones are pruned after a new
/// one lands.
pub const SNAPSHOT_LIMIT: usize = 30;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub name: String,
    pub created_at: u64,
    pub bytes: u64,
    /// Characters in the snapshot, the number writers actually care about.
    pub chars: usize,
    pub excerpt: String,
}

/// FNV-1a of the document id, hex encoded. Ids are path-like, so hashing keeps
/// the directory name safe on every platform.
fn key(id: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in id.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

pub fn directory(root: &Path, id: &str) -> PathBuf {
    root.join(".ruwriter").join("snapshots").join(key(id))
}

fn timestamped_name(millis: u64) -> String {
    format!("{millis:013}.txt")
}

fn excerpt(text: &str) -> String {
    let mut out = String::new();
    for character in text.chars() {
        if character == '\n' || character == '\r' {
            if !out.ends_with(' ') {
                out.push(' ');
            }
        } else {
            out.push(character);
        }
        if out.chars().count() >= 60 {
            break;
        }
    }
    out.trim().to_owned()
}

fn millis(seconds: u64) -> u64 {
    seconds.saturating_mul(1000)
}

fn seconds_from_name(name: &str) -> Option<u64> {
    name.strip_suffix(".txt")?
        .parse::<u64>()
        .ok()
        .map(|value| value / 1000)
}

fn names(root: &Path, id: &str) -> Vec<String> {
    let directory = directory(root, id);
    let Ok(entries) = fs::read_dir(&directory) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            seconds_from_name(&name).map(|_| name)
        })
        .collect();
    names.sort_by(|left, right| right.cmp(left));
    names
}

/// Timestamp of the newest snapshot. Used when opening a document, where
/// reading every snapshot's text just to know its age would be wasteful.
pub fn latest_time(root: &Path, id: &str) -> Option<u64> {
    let name = names(root, id).into_iter().next()?;
    seconds_from_name(&name)
}

pub fn list(root: &Path, id: &str) -> Vec<Snapshot> {
    let directory = directory(root, id);
    names(root, id)
        .into_iter()
        .filter_map(|name| {
            let created_at = seconds_from_name(&name)?;
            let bytes = fs::metadata(directory.join(&name)).ok()?.len();
            let text = fs::read_to_string(directory.join(&name)).ok()?;
            Some(Snapshot {
                name,
                created_at,
                bytes,
                chars: text.chars().count(),
                excerpt: excerpt(&text),
            })
        })
        .collect()
}

pub fn read(root: &Path, id: &str, name: &str) -> io::Result<String> {
    if !name.ends_with(".txt") || name.contains(['/', '\\']) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid snapshot name",
        ));
    }
    fs::read_to_string(directory(root, id).join(name))
}

/// Stores `text` unless the newest snapshot already holds exactly the same
/// text. Returns `None` when nothing changed, which keeps autosave-adjacent
/// snapshots from filling the folder with duplicates.
pub fn create(root: &Path, id: &str, text: &str, at: u64) -> io::Result<Option<Snapshot>> {
    let directory = directory(root, id);
    if let Some(latest) = names(root, id).into_iter().next()
        && fs::read_to_string(directory.join(&latest)).is_ok_and(|previous| previous == text)
    {
        return Ok(None);
    }
    fs::create_dir_all(&directory)?;
    let mut name = timestamped_name(millis(at));
    if directory.join(&name).exists() {
        name = timestamped_name(millis(at).saturating_add(1));
    }
    write_document(&directory.join(&name), text)?;
    prune(root, id);
    let bytes = fs::metadata(directory.join(&name))
        .map(|meta| meta.len())
        .unwrap_or(0);
    Ok(Some(Snapshot {
        name,
        created_at: at,
        bytes,
        chars: text.chars().count(),
        excerpt: excerpt(text),
    }))
}

fn prune(root: &Path, id: &str) {
    let directory = directory(root, id);
    let mut ordered = names(root, id);
    ordered.reverse();
    let excess = ordered.len().saturating_sub(SNAPSHOT_LIMIT);
    for name in ordered.into_iter().take(excess) {
        let _ = fs::remove_file(directory.join(name));
    }
}

/// Moves a document's snapshots when its id changes on rename. A failure here
/// must not block the rename; old snapshots simply stay under the old key.
pub fn move_on_rename(root: &Path, old_id: &str, new_id: &str) {
    let from = directory(root, old_id);
    let to = directory(root, new_id);
    if from.exists() && !to.exists() {
        let _ = fs::create_dir_all(to.parent().unwrap_or(&to));
        let _ = fs::rename(from, to);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshots_are_listed_newest_first_and_skip_duplicates() {
        let root = tempfile::tempdir().expect("root");
        create(root.path(), "a.txt", "first", 1_000).expect("first");
        create(root.path(), "a.txt", "second", 1_001).expect("second");
        assert!(
            create(root.path(), "a.txt", "second", 1_002)
                .expect("dup")
                .is_none()
        );
        let listed = list(root.path(), "a.txt");
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].created_at, 1_001);
        assert_eq!(
            read(root.path(), "a.txt", &listed[1].name).unwrap(),
            "first"
        );
    }

    #[test]
    fn old_snapshots_are_pruned() {
        let root = tempfile::tempdir().expect("root");
        for second in 0..(SNAPSHOT_LIMIT as u64 + 5) {
            create(root.path(), "a.txt", &format!("v{second}"), second).expect("create");
        }
        assert_eq!(list(root.path(), "a.txt").len(), SNAPSHOT_LIMIT);
    }
}
