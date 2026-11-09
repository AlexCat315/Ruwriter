//! Document identity and title derivation. A document is a Markdown file on
//! disk; the library index only records where it lives.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub const DOCUMENT_EXTENSION: &str = "md";
const TITLE_LIMIT: usize = 48;
const EXCERPT_LIMIT: usize = 120;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    pub id: String,
    pub title: String,
    pub path: PathBuf,
    /// Opened from outside the library; saving keeps the original path.
    pub external: bool,
    pub updated_at: u64,
    #[serde(default)]
    pub created_at: u64,
    /// Kept above unpinned documents in every list.
    #[serde(default)]
    pub pinned: bool,
    /// In the trash: hidden from the library until restored or purged.
    #[serde(default)]
    pub trashed: bool,
    /// Plain-text preview of the body, refreshed on every write.
    #[serde(default)]
    pub excerpt: String,
    #[serde(default)]
    pub words: usize,
}

impl Document {
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        path: PathBuf,
        external: bool,
    ) -> Self {
        let now = now();
        Self {
            id: id.into(),
            title: title.into(),
            path,
            external,
            updated_at: now,
            created_at: now,
            pinned: false,
            trashed: false,
            excerpt: String::new(),
            words: 0,
        }
    }

    /// Refreshes everything derived from the text. External files keep the
    /// title chosen at import unless the text supplies a heading.
    pub fn refresh(&mut self, text: &str, fallback: &str) {
        if !self.external {
            self.title = title_from_text(text, fallback);
        }
        self.excerpt = excerpt_from_text(text);
        self.words = crate::stats::count(text).words;
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

/// The body after the title line, with Markdown markers removed and
/// whitespace collapsed, cut on a char boundary.
pub fn excerpt_from_text(text: &str) -> String {
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    let _title = lines.next();
    let mut excerpt = String::new();
    let mut count = 0;
    for line in lines {
        if line.starts_with("```") || line.starts_with("---") || line.starts_with("***") {
            continue;
        }
        let stripped = strip_markers(line);
        if stripped.is_empty() {
            continue;
        }
        if !excerpt.is_empty() {
            excerpt.push(' ');
            count += 1;
        }
        for character in stripped.chars() {
            if count >= EXCERPT_LIMIT {
                return excerpt.trim_end().to_owned();
            }
            excerpt.push(character);
            count += 1;
        }
    }
    excerpt
}

fn strip_markers(line: &str) -> String {
    let mut rest = line.trim_start_matches(['#', '>', ' ']);
    for prefix in ["- [ ] ", "- [x] ", "- ", "* ", "+ "] {
        if let Some(stripped) = rest.strip_prefix(prefix) {
            rest = stripped;
            break;
        }
    }
    if let Some(position) = rest.find(". ")
        && position <= 3
        && rest[..position].chars().all(|c| c.is_ascii_digit())
    {
        rest = &rest[position + 2..];
    }
    let mut out = String::with_capacity(rest.len());
    let mut chars = rest.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '*' | '_' | '`' | '~' => {}
            '!' if chars.peek() == Some(&'[') => {}
            '[' => {}
            ']' => {
                // Drop a link target: `](...)`.
                if chars.peek() == Some(&'(') {
                    for next in chars.by_ref() {
                        if next == ')' {
                            break;
                        }
                    }
                }
            }
            _ => out.push(character),
        }
    }
    out.trim().to_owned()
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
