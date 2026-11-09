//! Document identity and title derivation. A document is a Markdown file on
//! disk; the library index only records where it lives.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub const DOCUMENT_EXTENSION: &str = "md";
const TITLE_LIMIT: usize = 48;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    pub id: String,
    pub title: String,
    pub path: PathBuf,
    /// Opened from outside the library; saving keeps the original path.
    pub external: bool,
    pub updated_at: u64,
}

impl Document {
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        path: PathBuf,
        external: bool,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            path,
            external,
            updated_at: now(),
        }
    }

    pub fn display_title(&self) -> &str {
        if self.title.trim().is_empty() {
            "未命名"
        } else {
            &self.title
        }
    }
}

/// Sortable, collision-resistant within one process and across restarts.
pub fn new_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}-{sequence:04x}", now_nanos())
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or_default()
}

/// Stable short suffix for generated filenames. The leading digits of a
/// nanosecond timestamp barely change, so hashing the whole ID avoids two new
/// documents sharing one path.
pub fn short_id(id: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in id.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:08x}")
}

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or_default()
}

/// First Markdown heading, or the first non-empty line, truncated on a char
/// boundary. Blank drafts keep the caller's fallback.
pub fn title_from_text(text: &str, fallback: &str) -> String {
    let first = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    let heading = first.trim_start_matches('#').trim();
    let title: String = if heading.is_empty() {
        first.chars().take(TITLE_LIMIT).collect()
    } else {
        heading.chars().take(TITLE_LIMIT).collect()
    };
    let title = title.trim().to_owned();
    if title.is_empty() {
        fallback.to_owned()
    } else {
        title
    }
}

/// Filesystem-safe stem; never produces separators or dot segments.
pub fn file_name_for(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || matches!(character, '-' | '_' | ' ') {
                character
            } else {
                '-'
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_matches('-').to_owned();
    if cleaned.is_empty() {
        "untitled".to_owned()
    } else {
        cleaned
    }
}
