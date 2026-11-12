//! The library is a folder of Markdown files and subfolders. Files are the
//! truth: the library rescans them and keeps only a cache of what it derived
//! (titles, excerpts, counts) plus what files cannot carry (pins, folder
//! order, expanded folders, where trashed files came from) in
//! `.ruwriter/state.json` inside the folder, so that state travels with it.
//!
//! The trash is the `.trash` folder. Hidden entries (names starting with `.`)
//! are never listed, which also keeps other tools' folders out of the way.

use crate::document::{Document, derive, file_stem, is_document_path};
use crate::files::{Stamp, read_document, stamp, write_document};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

pub const STATE_DIRECTORY: &str = ".ruwriter";
pub const STATE_FILE: &str = "state.json";
pub const TRASH_DIRECTORY: &str = ".trash";
const STATE_VERSION: u32 = 2;

/// What the document list shows when no folder is selected.
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub sidebar: bool,
    pub preview: bool,
    pub appearance: AppearancePreference,
    pub collection: Collection,
    /// A selected folder replaces the collection in the document list.
    pub folder: Option<String>,
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
            folder: None,
            font_size: 17,
        }
    }
}

/// Cached facts about one file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Meta {
    pub stamp: Stamp,
    pub title: String,
    pub excerpt: String,
    pub words: usize,
    pub created_at: u64,
    pub pinned: bool,
    /// Ruwriter named the file (`未命名.md`); saving renames it after its title.
    pub auto_named: bool,
}

/// The state file inside the library folder.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LibraryState {
    pub version: u32,
    /// Keyed by document id (path relative to the library folder).
    pub entries: BTreeMap<String, Meta>,
    /// Folder id → child folder names in the order the user arranged.
    pub order: BTreeMap<String, Vec<String>>,
    pub expanded: BTreeSet<String>,
    /// Trashed document id → the id it had before.
    pub trash_origins: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folder {
    /// Path relative to the library folder, `/`-separated.
    pub id: String,
    pub name: String,
    /// `""` for a top-level folder.
    pub parent: String,
    pub depth: usize,
}

pub struct Library {
    pub root: PathBuf,
    pub state: LibraryState,
    /// Every listed document, library and external, trashed included.
    pub documents: Vec<Document>,
    /// Folders depth-first in display order, without the library folder.
    pub folders: Vec<Folder>,
    /// Files opened from outside the library, keyed by absolute path.
    pub external: BTreeMap<String, Meta>,
}

impl Library {
    /// Opens `root`, creating it when missing, and scans it.
    pub fn open(root: &Path, external: BTreeMap<String, Meta>) -> Self {
        let _ = std::fs::create_dir_all(root);
        let state = std::fs::read(state_path(root))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let mut library = Self {
            root: root.to_path_buf(),
            state,
            documents: Vec::new(),
            folders: Vec::new(),
            external,
        };
        library.scan();
        library
    }

    pub fn save_state(&mut self) -> io::Result<()> {
        self.state.version = STATE_VERSION;
        let bytes = serde_json::to_vec_pretty(&self.state).map_err(io::Error::other)?;
        let json = String::from_utf8(bytes).map_err(io::Error::other)?;
        write_document(&state_path(&self.root), &json)
    }

    // MARK: - Identity

    /// The id of a path inside the library, `None` outside it.
    pub fn id_for(&self, path: &Path) -> Option<String> {
        let relative = path.strip_prefix(&self.root).ok()?;
        let parts: Vec<String> = relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy().into_owned())
            .collect();
        (!parts.is_empty()).then(|| parts.join("/"))
    }

    /// The absolute path of a library id; external ids already are one.
    pub fn path_for(&self, id: &str) -> PathBuf {
        if self.external.contains_key(id) {
            return PathBuf::from(id);
        }
        id.split('/')
            .filter(|part| !part.is_empty())
            .fold(self.root.clone(), |path, part| path.join(part))
    }

    pub fn trash_path(&self) -> PathBuf {
        self.root.join(TRASH_DIRECTORY)
    }

    pub fn find(&self, id: &str) -> Option<&Document> {
        self.documents.iter().find(|document| document.id == id)
    }

    pub fn folder(&self, id: &str) -> Option<&Folder> {
        self.folders.iter().find(|folder| folder.id == id)
    }

    /// The folder and every folder inside it.
    pub fn is_within(id: &str, folder: &str) -> bool {
        folder.is_empty() || id == folder || id.starts_with(&format!("{folder}/"))
    }

    // MARK: - Queries

    /// Documents in `folder` (direct children) or, without one, in
    /// `collection`; filtered by `query` over title and excerpt; pinned first,
    /// then most recently edited.
    pub fn list(&self, collection: Collection, folder: Option<&str>, query: &str) -> Vec<Document> {
        let now = crate::document::now();
        let needle = query.trim().to_lowercase();
        let mut documents: Vec<Document> = self
            .documents
            .iter()
            .filter(|document| match folder {
                Some(folder) => !document.trashed && document.folder.as_deref() == Some(folder),
                None => collection.contains(document, now),
            })
            .filter(|document| {
                needle.is_empty()
                    || document.title.to_lowercase().contains(&needle)
                    || document.excerpt.to_lowercase().contains(&needle)
            })
            .cloned()
            .collect();
        let trash = folder.is_none() && collection == Collection::Trash;
        documents.sort_by(|left, right| {
            let pinned = |document: &Document| !trash && document.pinned;
            pinned(right)
                .cmp(&pinned(left))
                .then_with(|| right.updated_at.cmp(&left.updated_at))
                .then_with(|| right.created_at.cmp(&left.created_at))
                .then_with(|| left.id.cmp(&right.id))
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

    /// Documents directly inside `folder`.
    pub fn folder_count(&self, folder: &str) -> usize {
        self.documents
            .iter()
            .filter(|document| !document.trashed && document.folder.as_deref() == Some(folder))
            .count()
    }

    /// Child folders of `parent` in display order.
    pub fn children(&self, parent: &str) -> Vec<Folder> {
        self.folders
            .iter()
            .filter(|folder| folder.parent == parent)
            .cloned()
            .collect()
    }

    // MARK: - Scanning

    /// Rebuilds the document and folder lists from disk. Unchanged files reuse
    /// the cache; changed ones are read again.
    pub fn scan(&mut self) {
        let mut seen = BTreeSet::new();
        let mut documents = Vec::new();
        let mut tree: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let root = self.root.clone();
        self.walk(&root, "", false, &mut seen, &mut documents, &mut tree);
        self.state.entries.retain(|id, _| seen.contains(id));
        self.state
            .trash_origins
            .retain(|id, _| seen.contains(id));

        let mut external = std::mem::take(&mut self.external);
        for (path, meta) in &mut external {
            let path_buf = PathBuf::from(path);
            if let Some(document) = refresh(path, &path_buf, None, false, meta) {
                documents.push(document);
            }
        }
        self.external = external;

        let mut folders = Vec::new();
        self.flatten(&tree, "", 0, &mut folders);
        let known: BTreeSet<String> = folders.iter().map(|folder| folder.id.clone()).collect();
        self.state.expanded.retain(|id| known.contains(id));
        self.state
            .order
            .retain(|parent, _| parent.is_empty() || known.contains(parent));
        self.folders = folders;
        self.documents = documents;
    }

    fn walk(
        &mut self,
        directory: &Path,
        relative: &str,
        trashed: bool,
        seen: &mut BTreeSet<String>,
        documents: &mut Vec<Document>,
        tree: &mut BTreeMap<String, Vec<String>>,
    ) {
        let Ok(entries) = std::fs::read_dir(directory) else {
            return;
        };
        let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            let id = join(relative, &name);
            let is_directory = entry.file_type().is_ok_and(|kind| kind.is_dir());
            if name.starts_with('.') {
                if is_directory && relative.is_empty() && name == TRASH_DIRECTORY && !trashed {
                    self.walk(&path, &id, true, seen, documents, tree);
                }
                continue;
            }
            if is_directory {
                if !trashed {
                    tree.entry(relative.to_owned()).or_default().push(name);
                }
                self.walk(&path, &id, trashed, seen, documents, tree);
            } else if is_document_path(&path) {
                let meta = self.state.entries.entry(id.clone()).or_default();
                if let Some(document) =
                    refresh(&id, &path, Some(relative.to_owned()), trashed, meta)
                {
                    seen.insert(id);
                    documents.push(document);
                }
            }
        }
    }

    fn flatten(
        &self,
        tree: &BTreeMap<String, Vec<String>>,
        parent: &str,
        depth: usize,
        out: &mut Vec<Folder>,
    ) {
        let Some(names) = tree.get(parent) else {
            return;
        };
        for name in self.ordered(parent, names) {
            let id = join(parent, &name);
            out.push(Folder {
                id: id.clone(),
                name,
                parent: parent.to_owned(),
                depth,
            });
            self.flatten(tree, &id, depth + 1, out);
        }
    }

    /// Arranged names first, in the user's order; the rest alphabetically.
    fn ordered(&self, parent: &str, names: &[String]) -> Vec<String> {
        let arranged = self.state.order.get(parent).cloned().unwrap_or_default();
        let mut result: Vec<String> = arranged
            .iter()
            .filter(|name| names.contains(name))
            .cloned()
            .collect();
        let mut rest: Vec<String> = names
            .iter()
            .filter(|name| !result.contains(name))
            .cloned()
            .collect();
        rest.sort_by_key(|name| name.to_lowercase());
        result.extend(rest);
        result
    }

    // MARK: - Bookkeeping for moves

    /// Moves cached state from one id to another, for a file or a folder and
    /// everything inside it.
    pub fn rekey(&mut self, from: &str, to: &str) {
        let moved = |id: &str| -> Option<String> {
            if id == from {
                Some(to.to_owned())
            } else {
                id.strip_prefix(&format!("{from}/"))
                    .map(|rest| format!("{to}/{rest}"))
            }
        };
        let entries = std::mem::take(&mut self.state.entries);
        self.state.entries = entries
            .into_iter()
            .map(|(id, meta)| (moved(&id).unwrap_or(id), meta))
            .collect();
        let order = std::mem::take(&mut self.state.order);
        self.state.order = order
            .into_iter()
            .map(|(id, names)| (moved(&id).unwrap_or(id), names))
            .collect();
        let expanded = std::mem::take(&mut self.state.expanded);
        self.state.expanded = expanded
            .into_iter()
            .map(|id| moved(&id).unwrap_or(id))
            .collect();
    }

    /// Places `name` in `parent`'s arranged order, before `before` when given.
    pub fn arrange(&mut self, parent: &str, name: &str, before: Option<&str>) {
        let names: Vec<String> = self
            .children(parent)
            .into_iter()
            .map(|folder| folder.name)
            .filter(|existing| existing != name)
            .collect();
        let index = before
            .and_then(|before| names.iter().position(|existing| existing == before))
            .unwrap_or(names.len());
        let mut names = names;
        names.insert(index, name.to_owned());
        self.state.order.insert(parent.to_owned(), names);
    }

    pub fn meta_mut(&mut self, id: &str) -> Option<&mut Meta> {
        if let Some(meta) = self.external.get_mut(id) {
            return Some(meta);
        }
        self.state.entries.get_mut(id)
    }
}

/// Re-derives a document when its file changed since the cache saw it.
fn refresh(
    id: &str,
    path: &Path,
    folder: Option<String>,
    trashed: bool,
    meta: &mut Meta,
) -> Option<Document> {
    let current = stamp(path)?;
    if meta.stamp != current || (meta.title.is_empty() && current.size > 0) {
        let text = read_document(path).unwrap_or_default();
        let derived = derive(&text, &file_stem(path), meta.auto_named);
        meta.title = derived.title;
        meta.excerpt = derived.excerpt;
        meta.words = derived.words;
        meta.stamp = current;
    }
    if meta.title.is_empty() {
        meta.title = file_stem(path);
    }
    if meta.created_at == 0 {
        meta.created_at = current.modified_seconds();
    }
    Some(Document {
        id: id.to_owned(),
        title: meta.title.clone(),
        path: path.to_path_buf(),
        external: folder.is_none(),
        folder,
        updated_at: current.modified_seconds(),
        created_at: meta.created_at,
        pinned: meta.pinned,
        trashed,
        excerpt: meta.excerpt.clone(),
        words: meta.words,
    })
}

pub fn join(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_owned()
    } else {
        format!("{parent}/{name}")
    }
}

/// The parent of an id; `""` for top-level entries.
pub fn parent_of(id: &str) -> &str {
    id.rsplit_once('/').map_or("", |(parent, _)| parent)
}

/// The last component of an id.
pub fn name_of(id: &str) -> &str {
    id.rsplit_once('/').map_or(id, |(_, name)| name)
}

pub fn state_path(root: &Path) -> PathBuf {
    root.join(STATE_DIRECTORY).join(STATE_FILE)
}
