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
pub mod sync;

pub use document::{DocStatus, Document, normalize_tags, parse_tags, title_from_text};
pub use library::{
    AppearancePreference, Collection, ColumnWidth, Folder, Library, LineSpacing, Settings,
    SortOrder, WritingMode,
};
pub use library::{
    CharacterCard, RelationshipCard, StoryProfile, WritingBrief, WritingFeedback, WritingMemory,
    WritingMemoryKind,
};
pub use outline::{Heading, heading_at, headings, is_chapter_line};
pub use session::{
    AiPreferences, AiSettings, MAX_PARALLEL_AI_MODELS, Rescan, Session, ViewOptions,
};
pub use snapshots::Snapshot;
pub use stats::{WritingStats, count};
pub use sync::{
    Manifest, ManifestDoc, SyncBase, SyncConfig, SyncDocument, SyncOutcome, SyncReport,
};
