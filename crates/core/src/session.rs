//! Editing session: which document is open, what is on disk, and what the
//! editor currently holds. Kept UI-free so every host shares the same rules.
//!
//! Structural operations that may change the open document leave the new
//! text in [`Session::text`]; callers compare [`Session::active_id`] before and
//! after to know whether the editor must reload.

use crate::document::{Document, now};
use crate::files::{read_document, write_document};
use crate::library::{self, AppearancePreference, Collection, Library, Settings};
use crate::paths;
use std::path::{Path, PathBuf};

const UNTITLED: &str = "未命名";
const FONT_SIZES: std::ops::RangeInclusive<u8> = 13..=26;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViewOptions {
    pub preview: bool,
    pub focus: bool,
    pub sidebar: bool,
}

pub struct Session {
    pub root: PathBuf,
    pub library: Library,
    pub active: Option<String>,
    /// Last text known to be on disk; the dirty baseline.
    pub saved: String,
    pub options: ViewOptions,
}

impl Session {
    pub fn load() -> Self {
        Self::at(&paths::data_root())
    }

    pub fn at(root: &Path) -> Self {
        let mut library = Library::load(root);
        backfill(&mut library);
        let settings = library.settings;
        let mut session = Self {
            root: root.to_path_buf(),
            library,
            active: None,
            saved: String::new(),
            options: ViewOptions {
                preview: settings.preview,
                focus: false,
                sidebar: settings.sidebar,
            },
        };
        let remembered = session
            .library
            .active
            .clone()
            .filter(|id| session.library.find(id).is_some_and(|d| !d.trashed));
        let fallback = || {
            session
                .library
                .list(Collection::All, "")
                .first()
                .map(|document| document.id.clone())
        };
        match remembered.or_else(fallback) {
            Some(id) => {
                let text = session
                    .library
                    .find(&id)
                    .and_then(|document| read_document(&document.path).ok())
                    .unwrap_or_default();
                session.active = Some(id.clone());
                session.library.active = Some(id);
                session.saved = text;
            }
            None => {
                let _ = session.activate_new();
            }
        }
        session
    }

    pub fn text(&self) -> String {
        self.saved.clone()
    }

    pub fn title(&self) -> String {
        self.active_document()
            .map(|document| document.display_title().to_owned())
            .unwrap_or_else(|| UNTITLED.to_owned())
    }

    /// Every document that is not in the trash, most recent first.
    pub fn documents(&self) -> Vec<Document> {
        self.library.list(Collection::All, "")
    }

    /// The current collection filtered by `query`.
    pub fn list(&self, query: &str) -> Vec<Document> {
        self.library.list(self.settings().collection, query)
    }

    pub fn count(&self, collection: Collection) -> usize {
        self.library.count(collection)
    }

    pub fn settings(&self) -> Settings {
        self.library.settings
    }

    pub fn active_id(&self) -> Option<String> {
        self.active.clone()
    }

    pub fn active_document(&self) -> Option<&Document> {
        self.active.as_deref().and_then(|id| self.library.find(id))
    }

    pub fn is_dirty(&self, text: &str) -> bool {
        text != self.saved
    }

    // MARK: - View settings

    pub fn toggle_preview(&mut self) {
        self.options.preview = !self.options.preview;
        self.library.settings.preview = self.options.preview;
        self.persist_settings();
    }

    pub fn toggle_focus(&mut self) {
        self.options.focus = !self.options.focus;
    }

    pub fn set_focus(&mut self, focus: bool) {
        self.options.focus = focus;
    }

    pub fn toggle_sidebar(&mut self) {
        self.options.sidebar = !self.options.sidebar;
        self.library.settings.sidebar = self.options.sidebar;
        self.persist_settings();
    }

    pub fn set_collection(&mut self, collection: Collection) {
        self.library.settings.collection = collection;
        self.persist_settings();
    }

    pub fn set_appearance(&mut self, appearance: AppearancePreference) {
        self.library.settings.appearance = appearance;
        self.persist_settings();
    }

    /// Steps the editor size by `delta` points within the supported range.
    pub fn adjust_font_size(&mut self, delta: i8) {
        let current = self.library.settings.font_size as i16;
        let next = (current + delta as i16)
            .clamp(*FONT_SIZES.start() as i16, *FONT_SIZES.end() as i16) as u8;
        self.library.settings.font_size = next;
        self.persist_settings();
    }

    pub fn reset_font_size(&mut self) {
        self.library.settings.font_size = Settings::default().font_size;
        self.persist_settings();
    }

    /// Settings are a convenience: failing to persist them never blocks work.
    fn persist_settings(&self) {
        let _ = self.library.save(&self.root);
    }

    // MARK: - Writing

    /// Writes every accepted edit. No debounce: quitting right after typing
    /// must not lose the last keystroke.
    pub fn autosave(&mut self, text: &str) -> Result<(), String> {
        if !self.is_dirty(text) {
            return Ok(());
        }
        self.write(text)
    }

    /// Explicit save also names a library file after its title and refreshes
    /// the library index on disk.
    pub fn save(&mut self, text: &str) -> Result<(), String> {
        self.write(text)?;
        self.rename_to_title();
        self.save_index()
    }

    /// Library-owned files follow their title (`标题-1a2b3c4d.md`) so they read
    /// well outside Ruwriter. External files keep the name the user gave them.
    /// A failed rename keeps the old name; the text is already safe.
    fn rename_to_title(&mut self) {
        let Some(id) = self.active.clone() else {
            return;
        };
        let Some(document) = self.library.find(&id) else {
            return;
        };
        if document.external {
            return;
        }
        let stem = format!(
            "{}-{}",
            crate::document::file_name_for(document.display_title()),
            crate::document::short_id(&id)
        );
        let target = document
            .path
            .with_file_name(format!("{stem}.{}", crate::document::DOCUMENT_EXTENSION));
        if target == document.path || target.exists() {
            return;
        }
        if std::fs::rename(&document.path, &target).is_ok()
            && let Some(document) = self.library.find_mut(&id)
        {
            document.path = target;
        }
    }

    fn save_index(&self) -> Result<(), String> {
        self.library
            .save(&self.root)
            .map_err(|error| error.to_string())
    }

    fn write(&mut self, text: &str) -> Result<(), String> {
        let id = self
            .active
            .clone()
            .ok_or_else(|| "没有打开的文档".to_owned())?;
        let path = self
            .library
            .find(&id)
            .map(|document| document.path.clone())
            .ok_or_else(|| "文档不存在".to_owned())?;
        write_document(&path, text).map_err(|error| error.to_string())?;
        self.saved = text.to_owned();
        if let Some(document) = self.library.find_mut(&id) {
            document.updated_at = now();
            document.refresh(text, UNTITLED);
        }
        Ok(())
    }

    // MARK: - Documents

    pub fn create(&mut self, current: &str) -> Result<String, String> {
        self.autosave(current)?;
        if matches!(
            self.settings().collection,
            Collection::Trash | Collection::Pinned
        ) {
            self.library.settings.collection = Collection::All;
        }
        self.activate_new()?;
        Ok(self.saved.clone())
    }

    fn activate_new(&mut self) -> Result<(), String> {
        let document =
            library::create_document(&self.root, "").map_err(|error| error.to_string())?;
        self.activate(document, String::new())
    }

    /// Opens a file from outside the library. A file already in the library
    /// is selected instead of being indexed twice.
    pub fn open_path(&mut self, path: &Path, current: &str) -> Result<String, String> {
        self.autosave(current)?;
        if let Some(existing) = self
            .library
            .documents
            .iter()
            .find(|document| document.path == path)
            .map(|document| document.id.clone())
        {
            if let Some(document) = self.library.find_mut(&existing) {
                document.trashed = false;
            }
            return self.select(&existing, current);
        }
        let (document, text) = library::import_document(path).map_err(|error| error.to_string())?;
        self.activate(document, text)?;
        Ok(self.saved.clone())
    }

    pub fn select(&mut self, id: &str, current: &str) -> Result<String, String> {
        self.autosave(current)?;
        if self.active.as_deref() == Some(id) {
            return Ok(self.saved.clone());
        }
        let path = self
            .library
            .find(id)
            .map(|document| document.path.clone())
            .ok_or_else(|| "文档不存在".to_owned())?;
        let text = read_document(&path).map_err(|error| error.to_string())?;
        self.active = Some(id.to_owned());
        self.library.active = Some(id.to_owned());
        self.saved = text.clone();
        self.save_index()?;
        Ok(text)
    }

    fn activate(&mut self, document: Document, text: String) -> Result<(), String> {
        self.library.insert(document.clone());
        self.active = Some(document.id.clone());
        self.library.active = Some(document.id);
        self.saved = text;
        self.save_index()
    }

    pub fn set_pinned(&mut self, id: &str, pinned: bool) -> Result<(), String> {
        let document = self
            .library
            .find_mut(id)
            .ok_or_else(|| "文档不存在".to_owned())?;
        document.pinned = pinned;
        self.save_index()
    }

    /// Moves a document to the trash. When it was open, the next document of
    /// the current list opens, or a blank one when nothing is left.
    pub fn trash(&mut self, id: &str, current: &str) -> Result<(), String> {
        self.autosave(current)?;
        let successor = self.successor(id);
        let document = self
            .library
            .find_mut(id)
            .ok_or_else(|| "文档不存在".to_owned())?;
        document.trashed = true;
        document.pinned = false;
        if self.active.as_deref() == Some(id) {
            self.replace_active(successor)?;
        }
        self.save_index()
    }

    pub fn restore(&mut self, id: &str) -> Result<(), String> {
        let document = self
            .library
            .find_mut(id)
            .ok_or_else(|| "文档不存在".to_owned())?;
        document.trashed = false;
        document.updated_at = now();
        self.save_index()
    }

    /// Permanently removes a trashed document. Library-owned files are
    /// deleted; external files stay on disk and only leave the index.
    pub fn purge(&mut self, id: &str) -> Result<(), String> {
        let successor = self.successor(id);
        let document = self
            .library
            .find(id)
            .cloned()
            .ok_or_else(|| "文档不存在".to_owned())?;
        if !document.trashed {
            return Err("只能删除废纸篓中的文档".to_owned());
        }
        if !document.external {
            match std::fs::remove_file(&document.path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.to_string()),
            }
        }
        self.library.remove(id);
        if self.active.as_deref() == Some(id) {
            self.replace_active(successor)?;
        }
        self.save_index()
    }

    pub fn empty_trash(&mut self) -> Result<(), String> {
        let trashed: Vec<String> = self
            .library
            .list(Collection::Trash, "")
            .into_iter()
            .map(|document| document.id)
            .collect();
        for id in trashed {
            self.purge(&id)?;
        }
        Ok(())
    }

    /// The document after `id` in the visible list, or before it at the end.
    fn successor(&self, id: &str) -> Option<String> {
        let list = self.list("");
        let index = list.iter().position(|document| document.id == id)?;
        list.get(index + 1)
            .or_else(|| index.checked_sub(1).and_then(|before| list.get(before)))
            .map(|document| document.id.clone())
    }

    fn replace_active(&mut self, successor: Option<String>) -> Result<(), String> {
        let next = successor
            .filter(|id| self.library.find(id).is_some_and(|d| !d.trashed))
            .or_else(|| self.documents().first().map(|d| d.id.clone()));
        match next {
            Some(id) => {
                let path = self
                    .library
                    .find(&id)
                    .map(|document| document.path.clone())
                    .ok_or_else(|| "文档不存在".to_owned())?;
                self.saved = read_document(&path).unwrap_or_default();
                self.active = Some(id.clone());
                self.library.active = Some(id);
                Ok(())
            }
            None => self.activate_new(),
        }
    }
}

/// Indexes written before excerpts and counts existed list documents with
/// neither; read each such file once so the list can show them.
fn backfill(library: &mut Library) {
    for document in &mut library.documents {
        if document.excerpt.is_empty()
            && document.words == 0
            && let Ok(text) = read_document(&document.path)
        {
            let fallback = document.title.clone();
            document.refresh(&text, &fallback);
        }
        if document.created_at == 0 {
            document.created_at = document.updated_at;
        }
    }
}
