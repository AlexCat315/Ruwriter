//! The document library: a JSON index plus the Markdown files it points at.

use crate::document::{
    DOCUMENT_EXTENSION, Document, file_name_for, new_id, short_id, title_from_text,
};
use crate::files::{read_document, write_document};
use crate::paths;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

const UNTITLED: &str = "未命名";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Library {
    #[serde(default)]
    pub documents: Vec<Document>,
    #[serde(default)]
    pub active: Option<String>,
}

impl Library {
    /// A missing or unreadable index starts a new library instead of aborting
    /// startup; the index is rebuilt by the next structural change.
    pub fn load(root: &Path) -> Self {
        let path = paths::library_index_path(root);
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, root: &Path) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        let json = String::from_utf8(bytes).map_err(io::Error::other)?;
        write_document(&paths::library_index_path(root), &json)
    }

    pub fn find(&self, id: &str) -> Option<&Document> {
        self.documents.iter().find(|document| document.id == id)
    }

    pub fn find_mut(&mut self, id: &str) -> Option<&mut Document> {
        self.documents.iter_mut().find(|document| document.id == id)
    }

    /// Most recently edited first.
    pub fn recent(&self) -> Vec<Document> {
        let mut documents = self.documents.clone();
        documents.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        documents
    }

    pub fn insert(&mut self, document: Document) {
        if let Some(index) = self
            .documents
            .iter()
            .position(|existing| existing.path == document.path)
        {
            self.documents[index] = document;
        } else {
            self.documents.push(document);
        }
    }

    pub fn remove(&mut self, id: &str) -> Option<Document> {
        let index = self
            .documents
            .iter()
            .position(|document| document.id == id)?;
        if self.active.as_deref() == Some(id) {
            self.active = None;
        }
        Some(self.documents.remove(index))
    }
}

/// Creates a library-owned Markdown file and its index entry.
pub fn create_document(root: &Path, text: &str) -> io::Result<Document> {
    let id = new_id();
    let title = title_from_text(text, UNTITLED);
    let stem = format!("{}-{}", file_name_for(&title), short_id(&id));
    let path = paths::documents_directory(root).join(format!("{stem}.{DOCUMENT_EXTENSION}"));
    write_document(&path, text)?;
    Ok(Document::new(id, title, path, false))
}

/// Reads a file outside the library; saving keeps writing to that same path.
pub fn import_document(path: &Path) -> io::Result<(Document, String)> {
    let text = read_document(path)?;
    let fallback = path
        .file_stem()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| UNTITLED.to_owned());
    let title = title_from_text(&text, &fallback);
    Ok((
        Document::new(new_id(), title, path.to_path_buf(), true),
        text,
    ))
}

/// Resolves the path backing the active document, if any.
pub fn active_path(library: &Library) -> Option<PathBuf> {
    library
        .active
        .as_deref()
        .and_then(|id| library.find(id))
        .map(|document| document.path.clone())
}
