mod ai;
mod ai_research;
mod app;
mod dates;
mod formatting;
pub mod gist;
mod markdown_syntax;
mod menu_bar;
mod model;
mod shortcuts;
mod theme;
mod views;

cove_ui::application!(launch = app::application);
