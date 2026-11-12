//! A document is a Markdown or text file. Library files are identified by
//! their path relative to the library folder (`小说/第一章.md`); files opened
//! from elsewhere by their absolute path. Everything else is derived from the
//! file and cached by the library.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub const DOCUMENT_EXTENSION: &str = "md";
/// Extensions the library lists as documents.
pub const DOCUMENT_EXTENSIONS: [&str; 3] = ["md", "markdown", "txt"];
pub const UNTITLED: &str = "未命名";
const TITLE_LIMIT: usize = 48;
const EXCERPT_LIMIT: usize = 120;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    pub id: String,
    pub title: String,
    pub path: PathBuf,
    /// Folder id inside the library (`""` is the library folder itself);
    /// `None` for files outside it.
    pub folder: Option<String>,
    /// Opened from outside the library; saving keeps the original path.
    pub external: bool,
    /// File modification time, seconds since the epoch.
    pub updated_at: u64,
    pub created_at: u64,
    /// Kept above unpinned documents in every list.
    pub pinned: bool,
    /// Lives in the library's trash folder.
    pub trashed: bool,
    /// Plain-text preview of the body.
    pub excerpt: String,
    pub words: usize,
}

impl Document {
    pub fn display_title(&self) -> &str {
        if self.title.trim().is_empty() {
            UNTITLED
        } else {
            &self.title
        }
    }

    /// The file name without its extension.
    pub fn stem(&self) -> String {
        file_stem(&self.path)
    }
}

pub fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub fn is_document_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            DOCUMENT_EXTENSIONS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(extension))
        })
}

/// What a file's text says about it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Derived {
    pub title: String,
    pub excerpt: String,
    pub words: usize,
}

/// Title, excerpt and word count for a file. Ruwriter-named drafts take their
/// title from the first line; any other file takes it from a leading heading,
/// or its file name, so notes written elsewhere keep the names given to them.
pub fn derive(text: &str, stem: &str, auto_named: bool) -> Derived {
    let first = first_line(text);
    let from_text = auto_named || first.starts_with('#');
    let title = if from_text {
        title_from_text(text, if stem.is_empty() { UNTITLED } else { stem })
    } else if stem.is_empty() {
        UNTITLED.to_owned()
    } else {
        stem.to_owned()
    };
    Derived {
        title,
        excerpt: excerpt_after(text, from_text && !first.is_empty()),
        words: crate::stats::count(text).words,
    }
}

fn first_line(text: &str) -> &str {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
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
    excerpt_after(text, true)
}

fn excerpt_after(text: &str, skip_title: bool) -> String {
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    if skip_title {
        let _title = lines.next();
    }
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
    let cleaned: String = cleaned.trim().trim_matches('-').chars().take(80).collect();
    let cleaned = cleaned.trim().to_owned();
    if cleaned.is_empty() {
        "untitled".to_owned()
    } else {
        cleaned
    }
}
