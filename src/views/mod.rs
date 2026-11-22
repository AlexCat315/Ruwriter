//! View tree. Panes are separate components so a keystroke recomposes only
//! the views that show what it changed.

mod ai;
mod editor;
mod format_text;
mod history;
mod info;
mod list;
mod outline;
mod palette;
mod rename;
mod root;
mod sidebar;
mod sync;
mod ui;

pub use root::RootView;
