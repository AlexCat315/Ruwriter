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
use cove_ui::compose::primitives::text::text_field::{
    TextFieldCaretState, TextFieldEditRequest, TextFieldNavigationRequest,
};
use cove_ui::composition::Scancode;
use cove_ui::core::Signal;
use cove_ui::core::scene::{
    AnimatorScheduler, AnyAnimationController, FilePicker, FilePickerFilter, FileSaveOutcome,
    FileSaveRequest, FileSaver, wake_scene_thread,
};
use cove_ui::platform::KeyEvent;
use ruwriter_core::document::file_name_for;
use ruwriter_core::sync::{SyncDocument, SyncReport};
use ruwriter_core::{
    AppearancePreference, Collection, DocStatus, Document, Heading, Rescan, Session, Snapshot,
    SortOrder, WritingStats, count, heading_at, headings, parse_tags,
};

use crate::ai::{AiState, ModelStatus, WritingTarget};
use crate::ai_research::{ManuscriptFile, ResearchRequest, ResearchScope};
use crate::formatting::{self, Command};

/// Documents longer than this save on a pause instead of on every keystroke;
/// rewriting a 200k-character file per key is what makes typing stutter.
const LARGE_DOCUMENT: usize = 40_000;
/// Quiet period before that write, in seconds.
const AUTOSAVE_DELAY: f64 = 0.3;
/// Caret changes are frequent; persist the remembered position once the
/// author pauses instead of writing `state.json` on every arrow key.
const CARET_SAVE_DELAY: f64 = 1.2;

/// What the rename dialog is renaming.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenameTarget {
    Folder(String),
    Document(String),
}

type DiskHandler = Rc<dyn Fn(Vec<PathBuf>)>;
type SyncHandler = Rc<dyn Fn(SyncEvent)>;

pub enum SyncEvent {
    Login(String),
    PreserveConflicts {
        library: PathBuf,
        library_id: String,
        documents: Vec<SyncDocument>,
        reply: std::sync::mpsc::SyncSender<Result<Vec<PathBuf>, String>>,
    },
    Finished {
        library: PathBuf,
        snapshot: Vec<SyncDocument>,
        conflicts: Vec<PathBuf>,
        result: Box<Result<SyncReport, String>>,
    },
}

thread_local! {
    static DISK_HANDLER: RefCell<Option<DiskHandler>> = const { RefCell::new(None) };
    static SYNC_HANDLER: RefCell<Option<SyncHandler>> = const { RefCell::new(None) };
    static DEFERRED: RefCell<Vec<Box<dyn FnOnce()>>> = const { RefCell::new(Vec::new()) };
}

/// Runs on the UI thread with the result of a background Gist sync.
pub fn deliver_sync_event(event: SyncEvent) {
    let handler = SYNC_HANDLER.with(|slot| slot.borrow().clone());
    if let Some(handler) = handler {
        handler(event);
    }
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
    /// Live caret and selection reported by the editor, in character units.
    pub caret: Signal<TextFieldCaretState>,
    /// One-shot focus/caret requests the editor applies after formatting.
    pub navigation: Signal<TextFieldNavigationRequest>,
    /// One-shot undoable edit the editor applies (formatting, replace, Enter).
    pub edit: Signal<Option<TextFieldEditRequest>>,
    pub find_open: Signal<bool>,
    pub find_query: Signal<TextBuffer>,
    pub find_replacement: Signal<TextBuffer>,
    /// Which match the caret currently sits on.
    pub find_index: Signal<usize>,
    /// One-shot request that moves typing into the find bar's query field.
    pub find_focus: Signal<u64>,
    /// The document whose synopsis, status and tags the info dialog edits.
    pub info_document: Signal<Option<String>>,
    pub info_synopsis: Signal<TextBuffer>,
    pub info_tags: Signal<TextBuffer>,
    pub info_status: Signal<DocStatus>,
    /// Bumped when a setting that changes the editor's look changed, so the
    /// field re-styles without being rebuilt (rebuilding would lose the
    /// caret and undo history).
    pub settings_revision: Signal<u64>,
    /// Text waiting for the debounced autosave of a long document.
    pub pending_save: Signal<Option<TextBuffer>>,
    /// Bumped by every edit that arms the autosave timer; a later edit makes
    /// the previous timer stale.
    pub save_revision: Signal<u64>,
    /// Bumped by every caret movement; a timer persists once typing settles.
    pub caret_revision: Signal<u64>,
    pub outline: Signal<Rc<Vec<Heading>>>,
    /// Whether the version-history dialog is open, and the snapshot list it
    /// shows. Loaded when opened, not on every keystroke.
    pub history_open: Signal<bool>,
    pub history_entries: Signal<Vec<Snapshot>>,
    /// Cloud backup through a private GitHub Gist.
    pub sync_open: Signal<bool>,
    pub sync_login: Signal<String>,
    pub sync_checking: Signal<bool>,
    pub sync_library: Signal<Option<PathBuf>>,
    pub sync_status: Signal<String>,
    pub sync_busy: Signal<bool>,
    pub sync_failed: Signal<bool>,
    pub ai: AiState,
}

fn project_folder(id: &str) -> &str {
    id.split_once('/').map_or("", |(folder, _)| folder)
}

fn markdown_filter() -> FilePickerFilter {
    FilePickerFilter::new(
        "Markdown / 文本",
        ["md".to_owned(), "markdown".to_owned(), "txt".to_owned()],
    )
}

fn text_filter() -> FilePickerFilter {
    FilePickerFilter::new("文本文件", ["txt".to_owned()])
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
                } else {
                    self.finish_text_summary(&current);
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
        self.pending_save.set(None);
        self.buffer.set(TextBuffer::new(&text));
        self.update_text_summary(&text);
        self.caret.set(TextFieldCaretState::INITIAL);
        self.navigation.set(TextFieldNavigationRequest::default());
        self.edit.set(None);
        self.find_open.set(false);
        self.find_index.set(0);
        self.document.update(|value| *value += 1);
    }

    // MARK: - Writing mode

    pub fn writing_mode(&self) -> ruwriter_core::WritingMode {
        self.settings().writing_mode
    }

    pub fn set_writing_mode(&self, mode: ruwriter_core::WritingMode) {
        let _ = self.with_session(|session| {
            session.set_writing_mode(mode);
            Ok(())
        });
        self.settings_revision.update(|value| *value += 1);
        self.listing.update(|value| *value += 1);
    }

    pub fn toggle_writing_mode(&self) {
        let mode = match self.writing_mode() {
            ruwriter_core::WritingMode::Novel => ruwriter_core::WritingMode::Markdown,
            ruwriter_core::WritingMode::Markdown => ruwriter_core::WritingMode::Novel,
        };
        self.set_writing_mode(mode);
    }

    pub fn paragraph_indent(&self) -> bool {
        self.settings().paragraph_indent
    }

    pub fn toggle_paragraph_indent(&self) {
        let _ = self.with_session(|session| {
            session.toggle_paragraph_indent();
            Ok(())
        });
        self.settings_revision.update(|value| *value += 1);
    }

    pub fn set_column_width(&self, width: ruwriter_core::ColumnWidth) {
        let _ = self.with_session(|session| {
            session.set_column_width(width);
            Ok(())
        });
        self.settings_revision.update(|value| *value += 1);
        let caret = self.caret.get();
        let anchor = caret.selected_range.map(|(start, _)| start);
        self.navigate(caret.caret_index, anchor, true);
    }

    pub fn line_spacing(&self) -> ruwriter_core::LineSpacing {
        self.settings().line_spacing
    }

    pub fn set_line_spacing(&self, spacing: ruwriter_core::LineSpacing) {
        let _ = self.with_session(|session| {
            session.set_line_spacing(spacing);
            Ok(())
        });
        self.settings_revision.update(|value| *value += 1);
        let caret = self.caret.get();
        let anchor = caret.selected_range.map(|(start, _)| start);
        self.navigate(caret.caret_index, anchor, true);
    }

    pub fn typewriter_scrolling(&self) -> bool {
        self.settings().typewriter_scrolling
    }

    pub fn toggle_typewriter_scrolling(&self) {
        let _ = self.with_session(|session| {
            session.toggle_typewriter_scrolling();
            Ok(())
        });
        self.settings_revision.update(|value| *value += 1);
        // Re-issue the caret so the new layout takes effect at once: turning
        // the mode on centres the current line, turning it off leaves it put.
        let caret = self.caret.get();
        let anchor = caret.selected_range.map(|(start, _)| start);
        self.navigate(caret.caret_index, anchor, true);
    }

    // MARK: - Editing

    /// Called by the editor after every accepted edit.
    pub fn on_change(&self, buffer: &TextBuffer) {
        if buffer.character_count() > LARGE_DOCUMENT {
            self.pending_save.set(Some(buffer.clone()));
            self.save_revision.update(|value| *value += 1);
            self.schedule_save();
            return;
        }
        self.pending_save.set(None);
        let text = buffer.string_value();
        self.update_text_summary(&text);
        self.write_text(text);
    }

    fn update_text_summary(&self, text: &str) {
        self.stats.set_if_changed(count(text));
        self.preview.set_if_changed(text.to_owned());
        self.outline.set_if_changed(Rc::new(headings(text)));
    }

    fn finish_text_summary(&self, text: &str) {
        if self.pending_save.get_untracked().is_some() {
            self.pending_save.set(None);
            self.update_text_summary(text);
        }
    }

    /// Writes the text and refreshes what depends on it.
    fn write_text(&self, text: String) {
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

    /// Writes the text the debounce has been holding, if any.
    fn flush_to_disk(&self) {
        let Some(buffer) = self.pending_save.get_untracked() else {
            return;
        };
        self.pending_save.set(None);
        let text = buffer.string_value();
        self.update_text_summary(&text);
        self.write_text(text);
    }

    fn schedule_caret_save(&self) {
        let model = self.clone();
        let revision = self.caret_revision.get_untracked();
        let validity = {
            let model = model.clone();
            move || model.caret_revision.get_untracked() == revision
        };
        let timer = AutosaveTimer {
            remaining: std::cell::Cell::new(CARET_SAVE_DELAY),
            valid: Box::new(validity),
            action: RefCell::new(Some(Box::new(move || {
                let _ = model.with_session(|session| {
                    session.persist_caret();
                    Ok(())
                });
            }))),
        };
        AnimatorScheduler::current().register(Rc::new(timer));
        wake_scene_thread();
    }

    fn schedule_save(&self) {
        let model = self.clone();
        let revision = self.save_revision.get_untracked();
        let validity = {
            let model = model.clone();
            move || {
                model.save_revision.get_untracked() == revision
                    && model.pending_save.get_untracked().is_some()
            }
        };
        let timer = AutosaveTimer {
            remaining: std::cell::Cell::new(AUTOSAVE_DELAY),
            valid: Box::new(validity),
            action: RefCell::new(Some(Box::new(move || model.flush_to_disk()))),
        };
        AnimatorScheduler::current().register(Rc::new(timer));
        wake_scene_thread();
    }

    /// The editor reports its caret and selection after every move.
    pub fn on_caret(&self, state: TextFieldCaretState) {
        if self.caret.get_untracked() == state {
            return;
        }
        self.caret.set(state);
        let offset = state.caret_index;
        let _ = self.with_session(|session| {
            session.remember_caret(offset);
            Ok(())
        });
        self.caret_revision.update(|value| *value += 1);
        self.schedule_caret_save();
    }

    /// Applies a Markdown formatting command at the last reported caret or
    /// selection. The edit goes through the field's undoable edit request, so
    /// Ctrl+Z steps back over it and the selection survives the wrap.
    pub fn format(&self, command: Command) {
        // Novel mode is plain text: inserting `**` or `#` would put markup
        // into the manuscript. Formatting is a Markdown-mode tool.
        if self.writing_mode() == ruwriter_core::WritingMode::Novel {
            return;
        }
        let buffer = self.buffer.get_untracked();
        let state = self.caret.get_untracked();
        let len = buffer.character_count();
        let selection = state
            .selected_range
            .map(|(low, high)| (low.min(len), high.min(len)));
        let text = buffer.string_value();
        let scalar_index = |index| {
            text[..buffer.utf8_offset_for_character_index(index)]
                .chars()
                .count()
        };
        let selection = selection.map(|(low, high)| (scalar_index(low), scalar_index(high)));
        let edit = formatting::apply(
            &text,
            scalar_index(state.caret_index.min(len)),
            selection,
            command,
        );
        let edited = TextBuffer::new(&edit.text);
        let grapheme_index = |index| {
            let byte = edit
                .text
                .char_indices()
                .nth(index)
                .map_or(edit.text.len(), |(byte, _)| byte);
            edited.character_index_for_utf8_offset(byte)
        };
        self.request_edit(
            0..len,
            edit.text.clone(),
            grapheme_index(edit.caret),
            edit.selection
                .map(|(low, high)| grapheme_index(if edit.caret == low { high } else { low })),
        );
    }

    /// Queues one undoable edit for the editor to apply on its next pass.
    fn request_edit(
        &self,
        range: std::ops::Range<usize>,
        replacement: String,
        caret: usize,
        anchor: Option<usize>,
    ) {
        let id = self.edit.get_untracked().as_ref().map_or(0, |edit| edit.id) + 1;
        self.edit.set(Some(TextFieldEditRequest {
            id,
            range,
            replacement,
            caret,
            anchor,
        }));
    }

    pub fn on_editor_key(&self, event: &KeyEvent) -> bool {
        let key = event.scancode;
        let modifiers = event.modifiers;
        if matches!(
            key,
            Scancode::RETURN
                | Scancode::KEYPAD_ENTER
                | Scancode::ESCAPE
                | Scancode::BACKSPACE
                | Scancode::DELETE
                | Scancode::TAB
                | Scancode::ARROW_LEFT
                | Scancode::ARROW_RIGHT
                | Scancode::ARROW_UP
                | Scancode::ARROW_DOWN
        ) && !modifiers.has_gui()
            && !modifiers.has_ctrl()
            && crate::shortcuts::has_composition()
        {
            return true;
        }
        // Cove accepts Shift+Primary+X as Cut unless the editor handles it first.
        if key == Scancode::X && modifiers.has_shift() && crate::shortcuts::has_primary(modifiers) {
            if !event.is_repeat {
                self.format(Command::Strikethrough);
            }
            return true;
        }
        if !matches!(key, Scancode::RETURN | Scancode::KEYPAD_ENTER)
            || !modifiers.is_empty()
            || self.writing_mode() != ruwriter_core::WritingMode::Markdown
        {
            return false;
        }
        let buffer = self.buffer.get_untracked();
        let state = self.caret.get_untracked();
        let Some(edit) =
            formatting::continue_line(&buffer, state.caret_index, state.selected_range)
        else {
            return false;
        };
        self.request_edit(edit.range, edit.replacement, edit.caret, None);
        true
    }

    /// Moves the caret (and optional selection anchor) without editing.
    /// `focus` also hands keyboard focus to the editor.
    pub fn navigate(&self, index: usize, anchor: Option<usize>, focus: bool) {
        let request = self
            .navigation
            .get_untracked()
            .caret_request_id
            .unwrap_or(0)
            + 1;
        self.navigation.set(TextFieldNavigationRequest {
            focus_request_id: focus.then_some(request),
            caret_request_id: Some(request),
            caret_request_index: Some(index),
            caret_request_anchor: anchor,
            caret_request_focus: focus,
        });
    }

    // MARK: - Find and replace

    pub fn find_matches(&self) -> Vec<(usize, usize)> {
        Self::matches_in(
            &self.buffer.get_untracked().string_value(),
            &self.find_query.get_untracked().string_value(),
        )
    }

    fn matches_in(text: &str, query: &str) -> Vec<(usize, usize)> {
        let needle: Vec<char> = query.chars().collect();
        if needle.is_empty() {
            return Vec::new();
        }
        let hay: Vec<char> = text.chars().collect();
        let mut matches = Vec::new();
        let mut at = 0;
        while at + needle.len() <= hay.len() {
            let hit = hay[at..at + needle.len()]
                .iter()
                .zip(&needle)
                .all(|(a, b)| a == b || a.eq_ignore_ascii_case(b));
            if hit {
                matches.push((at, at + needle.len()));
                at += needle.len();
            } else {
                at += 1;
            }
        }
        matches
    }

    pub fn open_find(&self) {
        let state = self.caret.get_untracked();
        if let Some((low, high)) = state.selected_range {
            let buffer = self.buffer.get_untracked();
            let len = buffer.character_count();
            let (low, high) = (low.min(len), high.min(len));
            if high > low && high - low <= 80 {
                self.find_query
                    .set(TextBuffer::new(&buffer.substring(low..high)));
            }
        }
        self.find_open.set(true);
        self.find_index.set(0);
        self.find_focus.set(self.find_focus.get_untracked() + 1);
        if let Some(&(low, high)) = self.find_matches().first() {
            self.navigate(low, Some(high), false);
        }
    }

    pub fn close_find(&self) {
        self.find_open.set(false);
        let caret = self.caret.get_untracked().caret_index;
        self.navigate(caret, None, true);
    }

    /// Escape closes the find bar first, then leaves focus mode.
    pub fn escape(&self) -> bool {
        if self.find_open.get_untracked() {
            self.close_find();
            return true;
        }
        self.leave_focus()
    }

    pub fn set_find_query(&self, query: &TextBuffer) {
        self.find_query.set(query.clone());
        self.find_index.set(0);
        if let Some(&(low, high)) = self.find_matches().first() {
            self.navigate(low, Some(high), false);
        }
    }

    pub fn find_step(&self, delta: i64) {
        if !self.find_open.get_untracked() {
            return;
        }
        let matches = self.find_matches();
        if matches.is_empty() {
            return;
        }
        let count = matches.len() as i64;
        let index = (self.find_index.get_untracked() as i64 + delta).rem_euclid(count) as usize;
        self.find_index.set(index);
        let (low, high) = matches[index];
        self.navigate(low, Some(high), false);
    }

    pub fn replace_one(&self) {
        let matches = self.find_matches();
        if matches.is_empty() {
            return;
        }
        let index = self.find_index.get_untracked().min(matches.len() - 1);
        let (low, high) = matches[index];
        let replacement = self.find_replacement.get_untracked().string_value();
        let buffer = self.buffer.get_untracked();
        let mut text = buffer.string_value();
        let chars: Vec<char> = text.chars().collect();
        text = chars[..low]
            .iter()
            .copied()
            .chain(replacement.chars())
            .chain(chars[high..].iter().copied())
            .collect();
        let caret = low + replacement.chars().count();
        self.request_edit(low..high, replacement, caret, None);
        let next = Self::matches_in(&text, &self.find_query.get_untracked().string_value());
        let index = next
            .iter()
            .position(|&(start, _)| start >= caret)
            .unwrap_or(0);
        self.find_index.set(index);
        if let Some(&(low, high)) = next.get(index) {
            self.navigate(low, Some(high), false);
        }
    }

    pub fn replace_all(&self) {
        let matches = self.find_matches();
        if matches.is_empty() {
            return;
        }
        let replacement = self.find_replacement.get_untracked().string_value();
        let buffer = self.buffer.get_untracked();
        let len = buffer.character_count();
        let mut text = buffer.string_value();
        for &(low, high) in matches.iter().rev() {
            let chars: Vec<char> = text.chars().collect();
            text = chars[..low]
                .iter()
                .copied()
                .chain(replacement.chars())
                .chain(chars[high..].iter().copied())
                .collect();
        }
        let caret = self
            .caret
            .get_untracked()
            .caret_index
            .min(text.chars().count());
        self.request_edit(0..len, text, caret, None);
        self.find_index.set(0);
    }

    // MARK: - Outline and plain prose

    /// Headings of the open document with their character offsets, for the
    /// palette's jump list.
    pub fn outline(&self) -> Vec<Heading> {
        headings(&self.buffer.get_untracked().string_value())
    }

    /// The heading tree plus the index of the section holding the caret.
    /// Reads both signals so the outline pane refreshes as you type and move.
    pub fn outline_at_caret(&self) -> (Rc<Vec<Heading>>, Option<usize>) {
        let heads = self.outline.get();
        let caret = self.caret.get().caret_index;
        let current = heading_at(&heads, caret);
        (heads, current)
    }

    /// Moves the caret to a heading and hands focus back to the editor.
    pub fn jump_to_heading(&self, offset: usize) {
        self.navigate(offset, None, true);
    }

    pub fn toggle_outline(&self) {
        let _ = self.with_session(|session| {
            session.toggle_outline();
            Ok(())
        });
        self.bump();
    }

    pub fn toggle_syntax(&self) {
        let _ = self.with_session(|session| {
            session.toggle_syntax();
            Ok(())
        });
        self.bump();
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
        let active_changed = self
            .read(|session| {
                session
                    .active_document()
                    .is_some_and(|doc| paths.contains(&doc.path))
            })
            .unwrap_or(false);
        if active_changed
            && self.pending_save.get_untracked().is_some()
            && let Err(error) = self.flush_sync_save()
        {
            self.notice.set(error);
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

    /// Installs this model as the receiver of background sync results.
    pub fn install_sync_handler(&self) {
        let model = self.clone();
        SYNC_HANDLER.with(|slot| {
            *slot.borrow_mut() = Some(Rc::new(move |event: SyncEvent| {
                model.on_sync_event(event);
            }));
        });
    }

    // MARK: - Writing assistant

    fn ai_document(&self) -> Option<(PathBuf, String, u64)> {
        self.read(|session| {
            let doc = session.active_document()?;
            (!doc.trashed).then(|| {
                (
                    session.library_root().to_path_buf(),
                    doc.id.clone(),
                    self.document.get_untracked(),
                )
            })
        })
        .flatten()
    }

    pub fn open_ai(&self) {
        if self.ai.open.get_untracked() {
            return;
        }
        let target = self.ai_document().map(|document| {
            let buffer = self.buffer.get_untracked();
            let len = buffer.character_count();
            let caret = self.caret.get_untracked();
            let selection = caret
                .selected_range
                .map(|(start, end)| start.min(len)..end.min(len))
                .filter(|range| !range.is_empty());
            let insertion = selection
                .as_ref()
                .map_or(caret.caret_index.min(len), |range| range.end);
            WritingTarget {
                document,
                buffer,
                selection,
                insertion,
            }
        });
        let context = target
            .as_ref()
            .map_or_else(|| Ok(TextBuffer::empty()), WritingTarget::context);
        self.ai.include_context.set(
            target
                .as_ref()
                .is_some_and(|target| target.selection.is_some())
                && context.as_ref().is_ok_and(|text| !text.is_empty()),
        );
        self.ai.context.set(context);
        self.ai.scope.set(ResearchScope::Excerpt);
        self.ai.viewed_source.set(None);
        self.ai.target.set(target);
        self.ai.prompt.set(TextBuffer::empty());
        self.ai.continuing.set(false);
        self.ai.error.set(String::new());
        self.ai.open.set(true);
        self.revision.update(|value| *value += 1);
    }

    pub fn close_ai(&self) {
        if self.ai.open.get_untracked() {
            self.ai.clear_batch();
            self.ai.target.set(None);
            self.ai.viewed_source.set(None);
            self.ai.context.set(Ok(TextBuffer::empty()));
            self.ai.prompt.set(TextBuffer::empty());
            self.ai.open.set(false);
            self.revision.update(|value| *value += 1);
        }
    }

    pub fn ai_settings(&self) -> ruwriter_core::AiSettings {
        let _ = self.revision.get();
        self.read(|session| session.ai_preferences().clone())
            .unwrap_or_default()
    }

    pub fn edit_ai_profile(&self, index: Option<usize>) {
        let settings = self.ai_settings();
        if let Some(index) = index {
            let Some(profile) = settings.models.get(index).cloned() else {
                return;
            };
            self.ai.edit_profile(Some(index), profile);
        } else {
            let endpoint = self.ai.endpoint.get_untracked().string_value();
            self.ai.edit_profile(
                None,
                ruwriter_core::AiPreferences {
                    endpoint,
                    ..Default::default()
                },
            );
        }
        self.ai.settings_open.set(true);
    }

    pub fn manage_ai(&self) {
        self.edit_ai_profile(self.ai_settings().selected.first().copied());
    }

    pub fn save_ai_preferences(&self) {
        let result = (|| {
            let address = self.ai.endpoint.get_untracked().string_value();
            let endpoint = cove_agent::openai_compatible::api_base(&address)
                .map_err(|error| error.to_string())?
                .to_owned();
            let model = self.ai.model.get_untracked().string_value();
            let profile = ruwriter_core::AiPreferences {
                endpoint,
                model: model.trim().to_owned(),
                tools: self.ai.tools.get_untracked(),
            };
            let key = self.ai.api_key.get_untracked().string_value();
            let key = key.trim();
            cove_agent::openai_compatible::ChatCompletionsModel::new(
                &profile.endpoint,
                &profile.model,
                Some(key),
            )
            .map_err(|error| error.to_string())?;
            let mut settings = self.ai_settings();
            let editing = self.ai.editing.get_untracked();
            if settings.models.iter().enumerate().any(|(index, saved)| {
                Some(index) != editing
                    && cove_agent::openai_compatible::api_base(&saved.endpoint).ok()
                        == Some(profile.endpoint.as_str())
                    && saved.model == profile.model
            }) {
                return Err("该服务下已经保存了这个模型。".to_owned());
            }
            let index = if let Some(index) = editing {
                let item = settings
                    .models
                    .get_mut(index)
                    .ok_or("模型已移除，请重新添加。")?;
                *item = profile.clone();
                index
            } else {
                let index = settings.models.len();
                settings.models.push(profile.clone());
                index
            };
            self.store_ai_settings(settings)?;
            self.ai.save_key(&profile.endpoint, key);
            self.ai.editing.set(Some(index));
            self.ai.endpoint.set(TextBuffer::new(&profile.endpoint));
            self.ai.model.set(TextBuffer::new(&profile.model));
            Ok(())
        })();
        self.ai.error.set(result.err().unwrap_or_default());
    }

    pub fn remove_ai_profile(&self, index: usize) {
        let mut settings = self.ai_settings();
        if index >= settings.models.len() {
            return;
        }
        settings.models.remove(index);
        settings.selected.retain(|&selected| selected != index);
        for selected in &mut settings.selected {
            if *selected > index {
                *selected -= 1;
            }
        }
        match self.store_ai_settings(settings) {
            Ok(()) => self.manage_ai(),
            Err(error) => self.ai.error.set(error),
        }
    }

    pub fn toggle_ai_profile(&self, index: usize) {
        let mut settings = self.ai_settings();
        if index >= settings.models.len() {
            return;
        }
        if let Some(position) = settings
            .selected
            .iter()
            .position(|&selected| selected == index)
        {
            if settings.selected.len() == 1 {
                return;
            }
            settings.selected.remove(position);
        } else {
            if settings.selected.len() >= ruwriter_core::MAX_PARALLEL_AI_MODELS {
                return;
            }
            settings.selected.push(index);
        }
        self.ai
            .error
            .set(self.store_ai_settings(settings).err().unwrap_or_default());
    }

    pub fn select_ai_profile(&self, index: usize) {
        let mut settings = self.ai_settings();
        if index < settings.models.len() && settings.selected.as_slice() != [index] {
            settings.selected = vec![index];
            self.ai
                .error
                .set(self.store_ai_settings(settings).err().unwrap_or_default());
        }
    }

    fn store_ai_settings(&self, mut settings: ruwriter_core::AiSettings) -> Result<(), String> {
        settings.normalize_selection();
        let tools = settings
            .selected
            .iter()
            .all(|&index| settings.models[index].tools);
        self.with_session(|session| session.set_ai_preferences(settings))?;
        if !tools {
            self.ai.scope.set(ResearchScope::Excerpt);
        }
        self.revision.update(|value| *value += 1);
        Ok(())
    }

    pub fn ai_project_folder(&self) -> Option<String> {
        self.read(|session| {
            session.active_document().and_then(|doc| {
                (!doc.external && !doc.trashed).then(|| project_folder(&doc.id).to_owned())
            })
        })
        .flatten()
    }

    pub fn run_ai(&self) {
        let result = (|| {
            let scope = self.ai.scope.get_untracked();
            let prepared = self
                .read(|session| {
                    let settings = session.ai_preferences();
                    let profiles: Vec<_> = settings
                        .selected
                        .iter()
                        .filter_map(|&index| settings.models.get(index).cloned())
                        .collect();
                    if scope == ResearchScope::Excerpt {
                        return Ok((profiles, None));
                    }
                    if profiles.iter().any(|profile| !profile.tools) {
                        return Err("跨章节检索需要为所有已选模型开启工具调用。".to_owned());
                    }
                    let target = self
                        .ai
                        .target
                        .get_untracked()
                        .ok_or_else(|| "请先打开要检索的文稿。".to_owned())?;
                    let doc = session
                        .active_document()
                        .filter(|doc| !doc.trashed)
                        .ok_or_else(|| "请先打开要检索的文稿。".to_owned())?;
                    let root = session.library_root();
                    if root != target.document.0
                        || doc.id != target.document.1
                        || self.document.get_untracked() != target.document.2
                        || self.buffer.get_untracked() != target.buffer
                    {
                        return Err("原文稿已变化，请重新打开助手确认检索范围。".to_owned());
                    }
                    if scope == ResearchScope::Project && doc.external {
                        return Err("库外文稿可检索本文稿。作品检索需要先打开库内文稿。".to_owned());
                    }
                    let folder = project_folder(&doc.id);
                    let files = session
                        .library
                        .documents
                        .iter()
                        .filter(|file| {
                            if scope == ResearchScope::Document {
                                file.id == doc.id
                            } else {
                                !file.external
                                    && !file.trashed
                                    && ruwriter_core::Library::is_within(&file.id, folder)
                            }
                        })
                        .map(|file| ManuscriptFile {
                            id: if file.external {
                                file.path
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .into_owned()
                            } else {
                                file.id.clone()
                            },
                            title: file.display_title().to_owned(),
                            path: file.path.clone(),
                        })
                        .collect();
                    let active_id = if doc.external {
                        doc.path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned()
                    } else {
                        doc.id.clone()
                    };
                    Ok((
                        profiles,
                        Some(ResearchRequest {
                            root: root.to_path_buf(),
                            files,
                            active_id,
                            active: target.buffer,
                            caret: target.insertion,
                        }),
                    ))
                })
                .ok_or_else(|| "无法读取模型设置。".to_owned())??;
            self.ai.start(prepared.0, prepared.1)
        })();
        self.ai.error.set(result.err().unwrap_or_default());
    }

    pub fn apply_ai(&self, index: usize, replace: bool) {
        let (Some(target), Some(batch)) = (
            self.ai.target.get_untracked(),
            self.ai.batch.get_untracked(),
        ) else {
            return;
        };
        let Some(ModelStatus::Ready(result)) = batch.runs.get(index).map(|run| &run.status) else {
            return;
        };
        let current = self.buffer.get_untracked();
        if self.ai_document().as_ref() != Some(&target.document) || current != target.buffer {
            self.ai
                .error
                .set("原文稿已变化。可复制结果，或重新打开助手选取范围。".to_owned());
            return;
        }
        let range = if replace {
            let Some(selection) = target.selection else {
                return;
            };
            selection
        } else {
            target.insertion..target.insertion
        };
        let text = result.text.string_value();
        let start = current.utf8_offset_for_character_index(range.start);
        let updated = current.replace(range.clone(), &text);
        let caret = updated.character_index_for_utf8_offset(start + text.len());
        self.close_ai();
        if self.options().preview {
            self.toggle_preview();
        }
        self.showing_editor.set(true);
        self.request_edit(range, text, caret, None);
        self.navigate(caret, None, true);
    }

    // MARK: - Cloud sync

    pub fn open_sync(&self) {
        let Some((library, last)) = self.read(|session| {
            (
                session.library_root().to_path_buf(),
                session.sync_config().last_synced_at,
            )
        }) else {
            return;
        };
        if self.sync_library.get_untracked().as_ref() != Some(&library) {
            self.sync_failed.set(false);
            self.sync_status.set(if last > 0 {
                format!("上次同步：{}", crate::dates::relative(last))
            } else {
                "尚未同步。首次同步会查找或创建私有 Gist。".to_owned()
            });
            self.sync_library.set(Some(library));
        }
        self.sync_open.set(true);
        self.revision.update(|value| *value += 1);
        self.check_sync_login();
    }

    pub fn close_sync(&self) {
        if self.sync_open.get_untracked() {
            self.sync_open.set(false);
            self.revision.update(|value| *value += 1);
        }
    }

    pub fn check_sync_login(&self) {
        if self.sync_checking.get_untracked() {
            return;
        }
        self.sync_checking.set(true);
        self.sync_login.set("正在检查 GitHub CLI 登录…".to_owned());
        let poster = cove_ui::core::ThreadPoster::current();
        std::thread::spawn(move || {
            let status = crate::gist::gh_status();
            poster.post(move || deliver_sync_event(SyncEvent::Login(status)));
        });
    }

    fn flush_sync_save(&self) -> Result<(), String> {
        let text = self.current_text();
        self.with_session(|session| session.autosave(&text))?;
        self.finish_text_summary(&text);
        if self.report_conflict() {
            self.load_active();
            self.bump();
        }
        Ok(())
    }

    pub fn sync_now(&self) {
        if self.sync_busy.get_untracked() {
            return;
        }
        let prepared = self.flush_sync_save().and_then(|()| {
            self.with_session(|session| {
                let local = session.sync_snapshot()?;
                session
                    .library
                    .save_state()
                    .map_err(|error| error.to_string())?;
                let config = session.sync_config();
                let request = crate::gist::SyncRequest {
                    gist_id: config.gist_id.clone(),
                    library_id: config.library_id.clone(),
                    device: ruwriter_core::sync::default_device(),
                    local,
                    base: config.base.clone(),
                };
                Ok((session.library_root().to_path_buf(), request))
            })
        });
        let (library, request) = match prepared {
            Ok(data) => data,
            Err(error) => {
                self.sync_status.set(format!("无法准备同步：{error}"));
                self.sync_failed.set(true);
                return;
            }
        };
        self.sync_library.set(Some(library.clone()));
        self.sync_busy.set(true);
        self.sync_failed.set(false);
        self.sync_status
            .set("正在连接 GitHub 并比对文稿…".to_owned());
        let poster = cove_ui::core::ThreadPoster::current();
        std::thread::spawn(move || {
            let mut conflicts = Vec::new();
            let result = (|| {
                let token = crate::gist::gh_token()?;
                let mut pending = crate::gist::prepare_sync(&request, &token)?;
                if !pending.report.conflicts.is_empty() {
                    let (reply, saved) = std::sync::mpsc::sync_channel(1);
                    let library = library.clone();
                    let library_id = request.library_id.clone();
                    let documents = std::mem::take(&mut pending.report.conflicts);
                    poster.post(move || {
                        deliver_sync_event(SyncEvent::PreserveConflicts {
                            library,
                            library_id,
                            documents,
                            reply,
                        })
                    });
                    conflicts = saved
                        .recv_timeout(std::time::Duration::from_secs(60))
                        .map_err(|_| "等待保存冲突副本超时，请重试。".to_owned())?
                        .map_err(|error| format!("无法先保存冲突副本：{error}"))?;
                }
                pending.finish(&token)
            })();
            poster.post(move || {
                deliver_sync_event(SyncEvent::Finished {
                    library,
                    snapshot: request.local,
                    conflicts,
                    result: Box::new(result),
                })
            });
        });
    }

    fn is_sync_library(&self, path: &Path) -> bool {
        self.read(|session| session.library_root() == path)
            .unwrap_or(false)
    }

    fn on_sync_event(&self, event: SyncEvent) {
        match event {
            SyncEvent::Login(status) => {
                self.sync_checking.set(false);
                self.sync_login.set(status);
            }
            SyncEvent::PreserveConflicts {
                library,
                library_id,
                documents,
                reply,
            } => {
                let result = self.with_session(|session| {
                    if session.library_root() != library
                        || session.sync_config().library_id != library_id
                    {
                        return Err("资料库已切换，请回到原资料库重试。".to_owned());
                    }
                    session.preserve_sync_conflicts(&documents)
                });
                self.listing.update(|value| *value += 1);
                let _ = reply.send(result);
            }
            SyncEvent::Finished {
                library,
                snapshot,
                mut conflicts,
                result,
            } => {
                self.sync_busy.set(false);
                if !self.is_sync_library(&library) {
                    self.sync_library.set(None);
                    if self.sync_open.get_untracked() {
                        self.open_sync();
                    }
                    return;
                }
                let applied = (*result).and_then(|report| {
                    self.flush_sync_save()
                        .map_err(|error| format!("同步后保存失败：{error}"))?;
                    let before = self.editor_identity();
                    let applied =
                        self.with_session(|session| session.apply_sync_report(&report, &snapshot));
                    if self.editor_identity() != before
                        || applied.as_ref().is_ok_and(|outcome| outcome.active_changed)
                    {
                        self.load_active();
                        self.restore_caret();
                    }
                    self.bump();
                    applied.map_err(|error| format!("同步结果写入失败：{error}"))
                });
                match applied {
                    Ok(outcome) => {
                        self.sync_failed.set(false);
                        let mut status = outcome.summary;
                        conflicts.extend(outcome.conflicts);
                        if !conflicts.is_empty() {
                            let names = conflicts
                                .iter()
                                .filter_map(|path| path.file_name())
                                .map(|name| name.to_string_lossy())
                                .collect::<Vec<_>>()
                                .join("、");
                            status.push_str(&format!("；已保留冲突副本：{names}"));
                        }
                        if outcome.deferred > 0 {
                            status.push_str(&format!(
                                "；同步期间有 {} 篇在本地修改，请再次同步。",
                                outcome.deferred
                            ));
                        }
                        self.sync_status.set(status);
                    }
                    Err(error) => {
                        self.sync_failed.set(true);
                        self.sync_status.set(error);
                    }
                }
            }
        }
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
                if self.report_conflict() {
                    self.pending_save.set(None);
                    let model = self.clone();
                    defer(move || {
                        model.load_active();
                        model.bump();
                    });
                } else {
                    self.finish_text_summary(&text);
                    self.notice.set_if_changed(String::new());
                    self.listing.update(|value| *value += 1);
                }
            }
            Err(error) => self.notice.set(error),
        }
    }

    // MARK: - Version history

    fn refresh_history(&self) {
        let entries = self.read(Session::snapshots).unwrap_or_default();
        self.history_entries.set(entries);
    }

    pub fn open_history(&self) {
        self.refresh_history();
        self.history_open.set(true);
    }

    pub fn close_history(&self) {
        self.history_open.set(false);
    }

    pub fn snapshot_now(&self) {
        let text = self.current_text();
        match self.with_session(|session| session.create_snapshot(&text)) {
            Ok(Some(_)) => self.notice.set("已保存当前版本".to_owned()),
            Ok(None) => self.notice.set("内容未变化，无需重复保存".to_owned()),
            Err(error) => self.notice.set(error),
        }
        self.refresh_history();
    }

    pub fn restore_snapshot(&self, name: &str) {
        let name = name.to_owned();
        let current = self.current_text();
        match self.with_session(|session| session.restore_snapshot(&name, &current)) {
            Ok(text) => {
                let length = current.chars().count();
                // An undoable whole-document edit keeps ⌘Z working after a
                // restore.
                self.request_edit(0..length, text, 0, None);
                self.notice
                    .set("已恢复历史版本，刚才的内容也存为快照".to_owned());
                self.refresh_history();
                self.history_open.set(false);
                self.bump();
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
        self.restore_caret();
    }

    pub fn open_path(&self, path: &Path) {
        let path = path.to_path_buf();
        self.structural(move |session, current| session.open_path(&path, current).map(drop));
        self.showing_editor.set(true);
        self.restore_caret();
    }

    /// Puts the caret back where the author stopped in this document. Applied
    /// once when a document opens and once on launch; an empty document just
    /// leaves the editor at its start.
    pub fn restore_caret(&self) {
        let characters = self.buffer.get_untracked().character_count();
        let offset = self
            .read(Session::active_caret)
            .unwrap_or(0)
            .min(characters);
        if offset > 0 {
            self.navigate(offset, None, true);
        }
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
        let novel = self.writing_mode() == ruwriter_core::WritingMode::Novel;
        let (suffix, filter) = if novel {
            ("txt", text_filter())
        } else {
            ("md", markdown_filter())
        };
        FileSaver::request(
            FileSaveRequest {
                suggested_name: format!("{}.{suffix}", file_name_for(&title)),
                filters: vec![filter],
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
                    model
                        .structural(move |session, current| session.open_library(&folder, current));
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

    // MARK: - Author metadata

    /// Opens the info dialog for `id`, pre-filled from the library state.
    pub fn begin_edit_info(&self, id: &str) {
        let (synopsis, tags, status) = self
            .read(|session| {
                session.library.find(id).map(|document| {
                    (
                        document.synopsis.clone(),
                        document.tags.clone(),
                        document.status,
                    )
                })
            })
            .flatten()
            .unwrap_or_default();
        self.info_synopsis.set(TextBuffer::new(&synopsis));
        self.info_tags.set(TextBuffer::new(&tags.join(", ")));
        self.info_status.set(status);
        self.info_document.set(Some(id.to_owned()));
    }

    pub fn edit_active_info(&self) {
        if let Some(id) = self.active_id() {
            self.begin_edit_info(&id);
        }
    }

    pub fn cancel_info(&self) {
        self.info_document.set(None);
    }

    pub fn set_info_status(&self, status: DocStatus) {
        self.info_status.set(status);
    }

    pub fn commit_info(&self) {
        let Some(id) = self.info_document.get_untracked() else {
            return;
        };
        let synopsis = self.info_synopsis.get_untracked().string_value();
        let tags = parse_tags(&self.info_tags.get_untracked().string_value());
        let status = self.info_status.get_untracked();
        self.info_document.set(None);
        self.structural(move |session, _| session.set_metadata(&id, &synopsis, status, &tags));
    }

    // MARK: - Renaming

    pub fn begin_rename(&self, target: RenameTarget) {
        let current = self
            .read(|session| match &target {
                RenameTarget::Folder(id) => {
                    session.library.folder(id).map(|folder| folder.name.clone())
                }
                RenameTarget::Document(id) => session.library.find(id).map(Document::stem),
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
            RenameTarget::Document(id) => self
                .structural(move |session, current| session.rename_document(&id, &name, current)),
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

    pub fn set_sort(&self, sort: SortOrder) {
        let _ = self.with_session(|session| {
            session.set_sort(sort);
            Ok(())
        });
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

/// One-shot scene timer that writes a long document once typing pauses.
struct AutosaveTimer {
    remaining: std::cell::Cell<f64>,
    valid: Box<dyn Fn() -> bool>,
    action: RefCell<Option<Box<dyn FnOnce()>>>,
}

impl AnyAnimationController for AutosaveTimer {
    fn is_finished(&self) -> bool {
        self.action.borrow().is_none() || !(self.valid)()
    }

    fn tick(&self, delta_time: f64) {
        if self.is_finished() {
            self.action.borrow_mut().take();
            return;
        }
        self.remaining.set(self.remaining.get() - delta_time);
        if self.remaining.get() <= 0.0 {
            self.finish_immediately();
        }
    }

    fn finish_immediately(&self) {
        let action = self.action.borrow_mut().take();
        if (self.valid)()
            && let Some(action) = action
        {
            action();
        }
    }

    fn cancel(&self) {
        self.action.borrow_mut().take();
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
