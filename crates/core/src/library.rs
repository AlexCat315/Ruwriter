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

/// What the document list shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Collection {
    #[default]
    All,
    Pinned,
    Recent,
    Trash,
}

impl Collection {
    pub const ALL: [Self; 4] = [Self::All, Self::Pinned, Self::Recent, Self::Trash];

    pub fn title(self) -> &'static str {
        match self {
            Self::All => "全部文稿",
            Self::Pinned => "置顶",
            Self::Recent => "最近 7 天",
            Self::Trash => "废纸篓",
        }
    }

    fn contains(self, document: &Document, now: u64) -> bool {
        match self {
            Self::All => !document.trashed,
            Self::Pinned => !document.trashed && document.pinned,
            Self::Recent => {
                !document.trashed && now.saturating_sub(document.updated_at) <= RECENT_SECONDS
            }
            Self::Trash => document.trashed,
        }
    }
}

const RECENT_SECONDS: u64 = 7 * 24 * 60 * 60;

/// Light / dark choice. `System` follows the platform when the host reports it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AppearancePreference {
    #[default]
    System,
    Light,
    Dark,
}

/// Window layout and appearance the user chose, restored on launch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub sidebar: bool,
    pub preview: bool,
    pub appearance: AppearancePreference,
    pub collection: Collection,
    /// Editor body size in points.
    pub font_size: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            sidebar: true,
            preview: false,
            appearance: AppearancePreference::System,
            collection: Collection::All,
            font_size: 17,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Library {
    #[serde(default)]
    pub documents: Vec<Document>,
    #[serde(default)]
    pub active: Option<String>,
    #[serde(default)]
    pub settings: Settings,
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

    /// Documents in `collection` whose title or excerpt contains `query`
    /// (case-insensitive), pinned first, then most recently edited.
    pub fn list(&self, collection: Collection, query: &str) -> Vec<Document> {
        let now = crate::document::now();
        let needle = query.trim().to_lowercase();
        let mut documents: Vec<Document> = self
            .documents
            .iter()
            .filter(|document| collection.contains(document, now))
            .filter(|document| {
                needle.is_empty()
                    || document.title.to_lowercase().contains(&needle)
                    || document.excerpt.to_lowercase().contains(&needle)
            })
            .cloned()
            .collect();
        documents.sort_by(|left, right| {
            let pinned = |document: &Document| collection != Collection::Trash && document.pinned;
            pinned(right)
                .cmp(&pinned(left))
                .then_with(|| right.updated_at.cmp(&left.updated_at))
                .then_with(|| right.id.cmp(&left.id))
        });
        documents
    }

    pub fn count(&self, collection: Collection) -> usize {
        let now = crate::document::now();
        self.documents
            .iter()
            .filter(|document| collection.contains(document, now))
            .count()
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
    let mut document = Document::new(id, title, path, false);
    document.refresh(text, UNTITLED);
    Ok(document)
}

/// Reads a file outside the library; saving keeps writing to that same path.
pub fn import_document(path: &Path) -> io::Result<(Document, String)> {
    let text = read_document(path)?;
    let fallback = path
        .file_stem()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| UNTITLED.to_owned());
    let title = title_from_text(&text, &fallback);
    let mut document = Document::new(new_id(), title, path.to_path_buf(), true);
    document.refresh(&text, &fallback);
    Ok((document, text))
}

/// Resolves the path backing the active document, if any.
pub fn active_path(library: &Library) -> Option<PathBuf> {
    library
        .active
        .as_deref()
        .and_then(|id| library.find(id))
        .map(|document| document.path.clone())
}
