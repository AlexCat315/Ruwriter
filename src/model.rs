//! Controller shared by every view.
//!
//! Signals are split by how often they change so a keystroke recomposes only
//! what shows it: `buffer`/`stats`/`preview` per edit, `listing` when the
//! document list must refresh, `revision` for structure (selection, layout,
//! settings), and `document` only when the editor must load another file.
//!
//! Files may change on disk at any time. The library watcher reports changed
//! paths through [`deliver_disk_change`]; the model rescans, reloads an open
//! document that changed, and reports what happened.

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use cove_ui::compose::primitives::text::TextBuffer;
use cove_ui::core::Signal;
use cove_ui::core::scene::{
    FilePicker, FilePickerFilter, FileSaveOutcome, FileSaveRequest, FileSaver,
};
use ruwriter_core::document::file_name_for;
use ruwriter_core::{
    AppearancePreference, Collection, Document, Rescan, Session, WritingStats, count,
};

/// What the rename dialog is renaming.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenameTarget {
    Folder(String),
    Document(String),
}

thread_local! {
    static DISK_HANDLER: RefCell<Option<Rc<dyn Fn(Vec<PathBuf>)>>> = const { RefCell::new(None) };
    static DEFERRED: RefCell<Vec<Box<dyn FnOnce()>>> = const { RefCell::new(Vec::new()) };
}

/// Runs on the UI thread with paths the library watcher saw change.
pub fn deliver_disk_change(paths: Vec<PathBuf>) {
    let handler = DISK_HANDLER.with(|slot| slot.borrow().clone());
    if let Some(handler) = handler {
        handler(paths);
    }
}

/// Runs `work` on the UI thread after the current event, outside any view
/// callback that is still on the stack.
pub fn defer(work: impl FnOnce() + 'static) {
    DEFERRED.with(|queue| queue.borrow_mut().push(Box::new(work)));
    cove_ui::core::ThreadPoster::current().post(|| {
        let jobs = DEFERRED.with(|queue| std::mem::take(&mut *queue.borrow_mut()));
        for job in jobs {
            job();
        }
    });
}

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
    pub rename: Signal<Option<RenameTarget>>,
    pub rename_text: Signal<TextBuffer>,
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

    /// The open document and how often its text was replaced from disk.
    fn editor_identity(&self) -> (Option<String>, u64) {
        self.read(|session| (session.active_id(), session.reloads))
            .unwrap_or_default()
    }

    /// Runs a structural operation, reloading the editor when it opened
    /// another document or the file's text replaced it.
    fn structural(&self, action: impl FnOnce(&mut Session, &str) -> Result<(), String>) {
        let before = self.editor_identity();
        let current = self.current_text();
        match self.with_session(|session| action(session, &current)) {
            Ok(()) => {
                self.report_conflict();
                if self.editor_identity() != before {
                    self.load_active();
                }
                self.notice.set_if_changed(String::new());
                self.bump();
            }
            Err(error) => self.notice.set(error),
        }
    }

    /// A write found the file changed elsewhere and kept the user's text in a
    /// copy: say where.
    fn report_conflict(&self) -> bool {
        let copy = self
            .with_session(|session| Ok(session.take_conflict()))
            .ok()
            .flatten();
        if let Some(copy) = copy {
            let name = copy
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            self.notice
                .set(format!("文件已在别处修改，你的版本另存为「{name}」"));
            true
        } else {
            false
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
                if self.report_conflict() {
                    // The editor is mid-event: swap its text afterwards.
                    let model = self.clone();
                    defer(move || {
                        model.load_active();
                        model.bump();
                    });
                } else {
                    self.notice.set_if_changed(String::new());
                }
                self.listing.update(|value| *value += 1);
            }
            Err(error) => self.notice.set(error),
        }
    }

    /// Files changed on disk. Ruwriter's own last write is recognised and
    /// skipped; anything else rescans the library and reloads the open
    /// document when its file changed.
    pub fn on_disk_change(&self, paths: &[PathBuf]) {
        if !self
            .read(|session| session.needs_rescan(paths))
            .unwrap_or(false)
        {
            return;
        }
        let Ok(outcome) = self.with_session(|session| Ok(session.rescan())) else {
            return;
        };
        match outcome {
            Rescan::Unchanged => {}
            Rescan::Reloaded => {
                self.load_active();
                self.notice.set("已载入其他应用的修改".to_owned());
            }
            Rescan::Replaced => {
                self.load_active();
                self.notice.set("文稿已在别处删除或移走".to_owned());
            }
        }
        self.bump();
    }

    /// Installs this model as the receiver of library watcher reports.
    pub fn install_disk_handler(&self) {
        let model = self.clone();
        DISK_HANDLER.with(|slot| {
            *slot.borrow_mut() = Some(Rc::new(move |paths: Vec<PathBuf>| {
                model.on_disk_change(&paths);
            }));
        });
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
        self.structural(move |session, _| session.restore(&id).map(drop));
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

    pub fn move_document(&self, id: &str, folder: &str) {
        let (id, folder) = (id.to_owned(), folder.to_owned());
        self.structural(move |session, current| session.move_document(&id, &folder, current));
    }

    // MARK: - Folders

    pub fn select_folder(&self, id: &str) {
        let _ = self.with_session(|session| {
            session.set_folder(id);
            Ok(())
        });
        self.showing_editor.set(false);
        self.bump();
    }

    /// Mirrors the sidebar's expanded folders into the library state.
    pub fn set_expanded(&self, expanded: HashSet<String>) {
        let changed = self.with_session(|session| {
            let current = session.library.state.expanded.clone();
            for id in current.iter().filter(|id| !expanded.contains(*id)) {
                session.set_expanded(id, false);
            }
            for id in expanded.iter().filter(|id| !current.contains(*id)) {
                session.set_expanded(id, true);
            }
            Ok(current != session.library.state.expanded)
        });
        if changed.unwrap_or(false) {
            self.listing.update(|value| *value += 1);
        }
    }

    /// Creates a folder inside `parent`, selects it and asks for its name.
    pub fn new_folder(&self, parent: &str) {
        let created = self.with_session(|session| session.create_folder(parent, "新建文件夹"));
        match created {
            Ok(id) => {
                self.select_folder(&id);
                self.begin_rename(RenameTarget::Folder(id));
            }
            Err(error) => self.notice.set(error),
        }
    }

    /// A new folder inside the selected one, or at the top of the library.
    pub fn new_folder_here(&self) {
        let parent = self.settings().folder.unwrap_or_default();
        self.new_folder(&parent);
    }

    /// Moves a folder into `parent`, before the sibling named `before`.
    pub fn move_folder(&self, id: &str, parent: &str, before: Option<&str>) {
        let (id, parent) = (id.to_owned(), parent.to_owned());
        let before = before.map(str::to_owned);
        self.structural(move |session, current| {
            session
                .move_folder(&id, &parent, before.as_deref(), current)
                .map(drop)
        });
    }

    pub fn trash_folder(&self, id: &str) {
        let id = id.to_owned();
        self.structural(move |session, current| session.trash_folder(&id, current));
    }

    /// Asks for a folder and opens it as the library.
    pub fn open_library_picker(&self) {
        let Some(handle) =
            cove_ui::hosts::desktop::application::composition::AppDisplayHandleHolder::current()
        else {
            self.notice.set("此平台不支持选择文件夹".to_owned());
            return;
        };
        let model = self.clone();
        handle.request_open_folder(
            None,
            Box::new(move |folder| {
                if let Some(folder) = folder {
                    let folder = PathBuf::from(folder);
                    model.structural(move |session, current| {
                        session.open_library(&folder, current)
                    });
                    model.showing_editor.set(true);
                }
            }),
        );
    }

    /// The library folder's name; the built-in folder reads as 我的文稿.
    pub fn library_name(&self) -> String {
        self.read(|session| {
            let root = session.library_root();
            if root == ruwriter_core::paths::documents_directory(&session.data_root) {
                return "我的文稿".to_owned();
            }
            root.file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "资料库".to_owned())
        })
        .unwrap_or_default()
    }

    // MARK: - Renaming

    pub fn begin_rename(&self, target: RenameTarget) {
        let current = self
            .read(|session| match &target {
                RenameTarget::Folder(id) => session
                    .library
                    .folder(id)
                    .map(|folder| folder.name.clone()),
                RenameTarget::Document(id) => {
                    session.library.find(id).map(Document::stem)
                }
            })
            .flatten()
            .unwrap_or_default();
        self.rename_text.set(TextBuffer::new(&current));
        self.rename.set(Some(target));
    }

    pub fn cancel_rename(&self) {
        self.rename.set(None);
    }

    pub fn commit_rename(&self) {
        let Some(target) = self.rename.get_untracked() else {
            return;
        };
        let name = self.rename_text.get_untracked().string_value();
        self.rename.set(None);
        match target {
            RenameTarget::Folder(id) => self.structural(move |session, current| {
                session.rename_folder(&id, &name, current).map(drop)
            }),
            RenameTarget::Document(id) => self.structural(move |session, current| {
                session.rename_document(&id, &name, current)
            }),
        }
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
