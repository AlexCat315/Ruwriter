//! View tree. Panes are separate components so a keystroke recomposes only
//! the views that show what it changed.

mod editor;
mod list;
mod palette;
mod rename;
mod root;
mod sidebar;
mod ui;

pub use root::RootView;
