//! Platform-independent document model for Ruwriter. This crate deliberately
//! has no UI dependency so it can be tested and reused by every Cove host.

pub mod document;
pub mod files;
pub mod library;
pub mod paths;
pub mod session;
pub mod stats;

pub use document::{Document, title_from_text};
pub use library::Library;
pub use session::{Session, ViewOptions};
pub use stats::{WritingStats, count};
