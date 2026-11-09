//! Editing session: which document is open, what is on disk, and what the
//! editor currently holds. Kept UI-free so every host shares the same rules.

use crate::document::{Document, now, title_from_text};
use crate::files::{read_document, write_document};
use crate::library::{self, Library};
use crate::paths;
use std::path::{Path, PathBuf};

const UNTITLED: &str = "未命名";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViewOptions {
    pub preview: bool,
    pub focus: bool,
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
        if library.documents.is_empty()
            && let Ok(document) = library::create_document(root, "")
        {
            library.active = Some(document.id.clone());
            library.insert(document);
            let _ = library.save(root);
        }
        let active = library.active.clone().or_else(|| {
            library
                .documents
                .first()
                .map(|document| document.id.clone())
        });
        let saved = active
            .as_deref()
            .and_then(|id| library.find(id))
            .and_then(|document| read_document(&document.path).ok())
            .unwrap_or_default();
        Self {
            root: root.to_path_buf(),
            library,
            active,
            saved,
            options: ViewOptions::default(),
        }
    }

    pub fn text(&self) -> String {
        self.saved.clone()
    }

    pub fn title(&self) -> String {
        self.active_document()
            .map(|document| document.display_title().to_owned())
            .unwrap_or_else(|| UNTITLED.to_owned())
    }

    pub fn documents(&self) -> Vec<Document> {
        self.library.recent()
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

    pub fn toggle_preview(&mut self) {
        self.options.preview = !self.options.preview;
    }

    pub fn toggle_focus(&mut self) {
        self.options.focus = !self.options.focus;
    }

    /// Writes every accepted edit. No debounce: quitting right after typing
    /// must not lose the last keystroke.
    pub fn autosave(&mut self, text: &str) -> Result<(), String> {
        if !self.is_dirty(text) {
            return Ok(());
        }
        self.write(text)
    }

    /// Explicit save also refreshes the library index on disk.
    pub fn save(&mut self, text: &str) -> Result<(), String> {
        self.write(text)?;
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
            if !document.external {
                document.title = title_from_text(text, UNTITLED);
            }
        }
        Ok(())
    }

    pub fn create(&mut self, current: &str) -> Result<String, String> {
        self.autosave(current)?;
        let document =
            library::create_document(&self.root, "").map_err(|error| error.to_string())?;
        self.activate(document, String::new())?;
        Ok(self.saved.clone())
    }

    pub fn open_path(&mut self, path: &Path, current: &str) -> Result<String, String> {
        self.autosave(current)?;
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
        self.library
            .save(&self.root)
            .map_err(|error| error.to_string())?;
        Ok(text)
    }

    fn activate(&mut self, document: Document, text: String) -> Result<(), String> {
        self.library.insert(document.clone());
        self.active = Some(document.id.clone());
        self.library.active = Some(document.id);
        self.saved = text;
        self.library
            .save(&self.root)
            .map_err(|error| error.to_string())
    }
}
