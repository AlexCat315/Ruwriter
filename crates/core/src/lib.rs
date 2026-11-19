//! Platform-independent document model for Ruwriter. This crate deliberately
//! has no UI dependency so it can be tested and reused by every Cove host.

pub mod document;
pub mod files;
pub mod library;
pub mod outline;
pub mod paths;
pub mod session;
pub mod snapshots;
pub mod stats;

pub use document::{DocStatus, Document, normalize_tags, parse_tags, title_from_text};
pub use library::{
    AppearancePreference, Collection, Folder, Library, Settings, SortOrder, WritingMode,
};
pub use outline::{Heading, heading_at, headings, is_chapter_line};
pub use session::{Rescan, Session, ViewOptions};
pub use snapshots::Snapshot;
pub use stats::{WritingStats, count};
