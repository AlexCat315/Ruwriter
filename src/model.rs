//! Controller shared by every view.
//!
//! Signals are split by how often they change so a keystroke recomposes only
//! what shows it: `buffer`/`stats`/`preview` per edit, `listing` when the
//! document list must refresh, `revision` for structure (selection, layout,
//! settings), and `document` only when the editor must load another file.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use cove_ui::compose::primitives::text::TextBuffer;
use cove_ui::core::Signal;
use cove_ui::core::scene::{
    FilePicker, FilePickerFilter, FileSaveOutcome, FileSaveRequest, FileSaver,
};
use ruwriter_core::document::file_name_for;
use ruwriter_core::{AppearancePreference, Collection, Document, Session, WritingStats, count};

#[derive(Clone)]
pub struct Model {
    pub session: Rc<RefCell<Session>>,
    pub revision: Signal<u64>,
    pub listing: Signal<u64>,
    pub document: Signal<u64>,
    pub buffer: Signal<TextBuffer>,
    pub preview: Signal<String>,
    pub stats: Signal<WritingStats>,
    pub notice: Signal<String>,
    pub query: Signal<TextBuffer>,
    pub palette: Signal<bool>,
    /// Single-pane layouts: whether the editor covers the document list.
    pub showing_editor: Signal<bool>,
}

fn markdown_filter() -> FilePickerFilter {
    FilePickerFilter::new(
        "Markdown / 文本",
        ["md".to_owned(), "markdown".to_owned(), "txt".to_owned()],
    )
}

impl Model {
    fn bump(&self) {
        self.revision.update(|value| *value += 1);
        self.listing.update(|value| *value += 1);
    }

    /// Reads the session without panicking if a menu callback re-enters.
    pub fn read<T>(&self, read: impl FnOnce(&Session) -> T) -> Option<T> {
        self.session.try_borrow().ok().map(|session| read(&session))
    }

    fn with_session<T>(
        &self,
        action: impl FnOnce(&mut Session) -> Result<T, String>,
    ) -> Result<T, String> {
        match self.session.try_borrow_mut() {
            Ok(mut session) => action(&mut session),
            Err(_) => Err("文档正在使用".to_owned()),
        }
    }

    fn current_text(&self) -> String {
        self.buffer.get_untracked().string_value()
    }

    fn active_id(&self) -> Option<String> {
        self.read(Session::active_id).flatten()
    }

    /// Runs a structural operation, reloading the editor when it opened
    /// another document.
    fn structural(&self, action: impl FnOnce(&mut Session, &str) -> Result<(), String>) {
        let before = self.active_id();
        let current = self.current_text();
        match self.with_session(|session| action(session, &current)) {
            Ok(()) => {
                if self.active_id() != before {
                    self.load_active();
                }
                self.notice.set_if_changed(String::new());
                self.bump();
            }
            Err(error) => self.notice.set(error),
        }
    }

    /// Puts the open document's saved text into the editor.
    fn load_active(&self) {
        let text = self.read(Session::text).unwrap_or_default();
        self.buffer.set(TextBuffer::new(&text));
        self.stats.set(count(&text));
        self.preview.set(text);
        self.document.update(|value| *value += 1);
    }

    // MARK: - Editing

    /// Called by the editor after every accepted edit.
    pub fn on_change(&self, buffer: &TextBuffer) {
        let text = buffer.string_value();
        self.stats.set_if_changed(count(&text));
        self.preview.set_if_changed(text.clone());
        match self.with_session(|session| session.autosave(&text)) {
            Ok(()) => {
                self.notice.set_if_changed(String::new());
                self.listing.update(|value| *value += 1);
            }
            Err(error) => self.notice.set(error),
        }
    }

    pub fn save(&self) {
        let text = self.current_text();
        match self.with_session(|session| session.save(&text)) {
            Ok(()) => {
                self.notice.set_if_changed(String::new());
                self.listing.update(|value| *value += 1);
            }
            Err(error) => self.notice.set(error),
        }
    }

    // MARK: - Documents

    pub fn create(&self) {
        self.structural(|session, current| session.create(current).map(drop));
        self.query.set(TextBuffer::empty());
        self.showing_editor.set(true);
    }

    pub fn select(&self, id: &str) {
        let id = id.to_owned();
        self.structural(move |session, current| session.select(&id, current).map(drop));
        self.showing_editor.set(true);
    }

    pub fn open_path(&self, path: &Path) {
        let path = path.to_path_buf();
        self.structural(move |session, current| session.open_path(&path, current).map(drop));
        self.showing_editor.set(true);
    }

    pub fn open_picker(&self) {
        let model = self.clone();
        FilePicker::request(
            &[markdown_filter()],
            true,
            Box::new(move |paths| {
                for path in paths {
                    model.open_path(Path::new(&path));
                }
            }),
        );
    }

    pub fn set_pinned(&self, id: &str, pinned: bool) {
        let id = id.to_owned();
        self.structural(move |session, _| session.set_pinned(&id, pinned));
    }

    pub fn trash(&self, id: &str) {
        let id = id.to_owned();
        self.structural(move |session, current| session.trash(&id, current));
    }

    pub fn trash_active(&self) {
        if let Some(id) = self.active_id() {
            self.trash(&id);
        }
    }

    pub fn restore(&self, id: &str) {
        let id = id.to_owned();
        self.structural(move |session, _| session.restore(&id));
    }

    pub fn purge(&self, id: &str) {
        let id = id.to_owned();
        self.structural(move |session, _| session.purge(&id));
    }

    pub fn empty_trash(&self) {
        self.structural(|session, _| session.empty_trash());
    }

    /// Writes the open document wherever the user chooses. Platforms without
    /// a save panel report it instead of pretending.
    pub fn export(&self) {
        self.save();
        let Some((title, text)) = self.read(|session| (session.title(), session.text())) else {
            return;
        };
        let notice = self.notice;
        FileSaver::request(
            FileSaveRequest {
                suggested_name: format!("{}.md", file_name_for(&title)),
                filters: vec![markdown_filter()],
                contents: text.into_bytes(),
            },
            Box::new(move |outcome| match outcome {
                FileSaveOutcome::Saved { .. } | FileSaveOutcome::Cancelled => {}
                FileSaveOutcome::Unsupported => notice.set("此平台不支持导出".to_owned()),
                FileSaveOutcome::Failed(error) => notice.set(error),
            }),
        );
    }

    // MARK: - Navigation and layout

    pub fn set_collection(&self, collection: Collection) {
        let _ = self.with_session(|session| {
            session.set_collection(collection);
            Ok(())
        });
        self.showing_editor.set(false);
        self.bump();
    }

    pub fn toggle_preview(&self) {
        let _ = self.with_session(|session| {
            session.toggle_preview();
            Ok(())
        });
        self.bump();
    }

    pub fn toggle_focus(&self) {
        let _ = self.with_session(|session| {
            session.toggle_focus();
            Ok(())
        });
        self.bump();
    }

    /// Escape leaves focus mode; returns whether it did anything.
    pub fn leave_focus(&self) -> bool {
        let focused = self.read(|session| session.options.focus).unwrap_or(false);
        if focused {
            self.toggle_focus();
        }
        focused
    }

    pub fn toggle_sidebar(&self) {
        let _ = self.with_session(|session| {
            if session.options.focus {
                session.set_focus(false);
                if !session.options.sidebar {
                    session.toggle_sidebar();
                }
            } else {
                session.toggle_sidebar();
            }
            Ok(())
        });
        self.bump();
    }

    pub fn show_list(&self) {
        self.showing_editor.set(false);
    }

    pub fn set_appearance(&self, appearance: AppearancePreference) {
        let _ = self.with_session(|session| {
            session.set_appearance(appearance);
            Ok(())
        });
        self.bump();
    }

    /// Cycles 跟随系统 → 浅色 → 深色.
    pub fn cycle_appearance(&self) {
        let next = match self.settings().appearance {
            AppearancePreference::System => AppearancePreference::Light,
            AppearancePreference::Light => AppearancePreference::Dark,
            AppearancePreference::Dark => AppearancePreference::System,
        };
        self.set_appearance(next);
    }

    pub fn adjust_font_size(&self, delta: i8) {
        let _ = self.with_session(|session| {
            session.adjust_font_size(delta);
            Ok(())
        });
        self.bump();
    }

    pub fn reset_font_size(&self) {
        let _ = self.with_session(|session| {
            session.reset_font_size();
            Ok(())
        });
        self.bump();
    }

    pub fn toggle_palette(&self) {
        self.palette.update(|open| *open = !*open);
    }

    // MARK: - Queries for views

    pub fn settings(&self) -> ruwriter_core::Settings {
        self.read(Session::settings).unwrap_or_default()
    }

    pub fn options(&self) -> ruwriter_core::ViewOptions {
        self.read(|session| session.options).unwrap_or_default()
    }

    pub fn active_document(&self) -> Option<Document> {
        self.read(|session| session.active_document().cloned())
            .flatten()
    }
}
