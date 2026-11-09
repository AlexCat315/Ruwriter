//! Controller shared by every view. Structural changes bump `revision`;
//! keystroke-only updates touch the stats/preview signals instead, so typing
//! never rebuilds the editor.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use cove_ui::compose::primitives::text::TextBuffer;
use cove_ui::core::Signal;
use cove_ui::core::scene::{FilePicker, FilePickerFilter};
use ruwriter_core::{Session, WritingStats, count};

#[derive(Clone)]
pub struct Actions {
    pub session: Rc<RefCell<Session>>,
    pub revision: Signal<u64>,
    pub buffer: Signal<TextBuffer>,
    pub preview: Signal<String>,
    pub stats: Signal<WritingStats>,
    pub notice: Signal<String>,
}

impl Actions {
    fn bump(&self) {
        self.revision.update(|value| *value += 1);
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

    fn apply_text(&self, text: String) {
        self.buffer.set(TextBuffer::new(&text));
        self.stats.set(count(&text));
        self.preview.set(text);
        self.notice.set(String::new());
        self.bump();
    }

    fn report(&self, result: Result<String, String>) {
        match result {
            Ok(text) => self.apply_text(text),
            Err(error) => self.notice.set(error),
        }
    }

    /// Called by `TextField` after every accepted edit.
    pub fn on_change(&self, buffer: &TextBuffer) {
        let text = buffer.string_value();
        self.stats.set_if_changed(count(&text));
        self.preview.set_if_changed(text.clone());

        let before = self
            .session
            .try_borrow()
            .map(|session| session.title())
            .unwrap_or_default();
        let result = self.with_session(|session| {
            session.autosave(&text)?;
            Ok(session.title())
        });
        match result {
            Ok(after) => {
                self.notice.set_if_changed(String::new());
                if before != after {
                    self.bump();
                }
            }
            Err(error) => self.notice.set(error),
        }
    }

    pub fn save(&self) {
        let text = self.buffer.get_untracked().string_value();
        match self.with_session(|session| session.save(&text)) {
            Ok(()) => {
                self.notice.set(String::new());
                self.bump();
            }
            Err(error) => self.notice.set(error),
        }
    }

    pub fn create(&self) {
        let current = self.buffer.get_untracked().string_value();
        self.report(self.with_session(|session| session.create(&current)));
    }

    pub fn open_path(&self, path: &Path) {
        let current = self.buffer.get_untracked().string_value();
        self.report(self.with_session(|session| session.open_path(path, &current)));
    }

    pub fn open_picker(&self) {
        let actions = self.clone();
        FilePicker::request(
            &[FilePickerFilter::new(
                "Markdown / 文本",
                ["md".to_owned(), "markdown".to_owned(), "txt".to_owned()],
            )],
            false,
            Box::new(move |paths| {
                if let Some(path) = paths.first() {
                    actions.open_path(Path::new(path));
                }
            }),
        );
    }

    pub fn select(&self, id: &str) {
        let current = self.buffer.get_untracked().string_value();
        self.report(self.with_session(|session| session.select(id, &current)));
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
}
