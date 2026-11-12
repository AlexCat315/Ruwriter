//! Editing session: which library folder is open, which document is being
//! edited, what is on disk, and the user's view settings. UI-free so every
//! host shares the same rules.
//!
//! Operations that may change the open document leave its text in
//! [`Session::text`]; callers compare [`Session::active_id`] and
//! [`Session::reloads`] before and after to know whether the editor must
//! reload.
//!
//! Files can change underneath Ruwriter (another editor, a sync client).
//! Every read and write records the file's [`Stamp`]; [`Session::rescan`]
//! reloads an open document that changed on disk, and a write that finds the
//! file changed since it was read keeps both versions instead of overwriting.

use crate::document::{
    DOCUMENT_EXTENSION, Document, UNTITLED, derive, file_name_for, file_stem, now,
};
use crate::files::{Stamp, move_path, read_document, stamp, unique_path, write_document};
use crate::library::{
    AppearancePreference, Collection, Library, Meta, Settings, name_of, parent_of,
};
use crate::paths;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const FONT_SIZES: std::ops::RangeInclusive<u8> = 13..=26;
const APP_STATE_VERSION: u32 = 2;
const NEW_FOLDER: &str = "新建文件夹";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViewOptions {
    pub preview: bool,
    pub focus: bool,
    pub sidebar: bool,
}

/// What a rescan did to the open document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rescan {
    Unchanged,
    /// It changed on disk and its new text was loaded.
    Reloaded,
    /// It disappeared; another document (or a new blank one) is open.
    Replaced,
}

/// Application state kept in the data folder: which library is open and the
/// user's settings. Version 1 also listed every document; it is migrated.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct AppState {
    version: u32,
    library_folder: Option<PathBuf>,
    active: Option<String>,
    settings: Settings,
    external: BTreeMap<String, Meta>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    documents: Vec<LegacyDocument>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct LegacyDocument {
    id: String,
    path: PathBuf,
    external: bool,
    created_at: u64,
    pinned: bool,
    trashed: bool,
}

pub struct Session {
    pub data_root: PathBuf,
    app: AppState,
    pub library: Library,
    pub active: Option<String>,
    /// Last text known to be on disk; the dirty baseline.
    pub saved: String,
    saved_stamp: Option<Stamp>,
    /// Counts replacements of the open document's text by its file.
    pub reloads: u64,
    /// Where the last write that found the file changed kept the user's text.
    pub conflict: Option<PathBuf>,
    pub options: ViewOptions,
}

impl Session {
    pub fn load() -> Self {
        Self::at(&paths::data_root())
    }

    pub fn at(data_root: &Path) -> Self {
        let mut app: AppState = std::fs::read(paths::library_index_path(data_root))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let root = app
            .library_folder
            .clone()
            .unwrap_or_else(|| paths::documents_directory(data_root));
        let legacy = std::mem::take(&mut app.documents);
        let external = std::mem::take(&mut app.external);
        let library = Library::open(&root, external);
        let settings = app.settings.clone();
        let mut session = Self {
            data_root: data_root.to_path_buf(),
            app,
            library,
            active: None,
            saved: String::new(),
            saved_stamp: None,
            reloads: 0,
            conflict: None,
            options: ViewOptions {
                preview: settings.preview,
                focus: false,
                sidebar: settings.sidebar,
            },
        };
        if !legacy.is_empty() {
            session.migrate(legacy);
        }
        if session
            .app
            .settings
            .folder
            .as_deref()
            .is_some_and(|folder| !folder.is_empty() && session.library.folder(folder).is_none())
        {
            session.app.settings.folder = None;
        }
        let remembered = session
            .app
            .active
            .clone()
            .filter(|id| session.library.find(id).is_some_and(|d| !d.trashed));
        let first = session.documents().first().map(|document| document.id.clone());
        match remembered.or(first) {
            Some(id) => session.open_document(&id),
            None => {
                let _ = session.activate_new("");
            }
        }
        session.persist();
        session
    }

    /// Version 1 kept every document in one folder and the trash as a flag.
    fn migrate(&mut self, legacy: Vec<LegacyDocument>) {
        let mut renamed: BTreeMap<String, String> = BTreeMap::new();
        for document in legacy {
            if document.external {
                let id = document.path.to_string_lossy().into_owned();
                self.library.external.insert(
                    id.clone(),
                    Meta {
                        pinned: document.pinned,
                        created_at: document.created_at,
                        ..Meta::default()
                    },
                );
                renamed.insert(document.id, id);
                continue;
            }
            let Some(mut id) = self.library.id_for(&document.path) else {
                continue;
            };
            if document.trashed {
                let destination = unique_path(
                    &self.library.trash_path(),
                    &file_stem(&document.path),
                    DOCUMENT_EXTENSION,
                    None,
                );
                if move_path(&document.path, &destination).is_ok()
                    && let Some(trashed) = self.library.id_for(&destination)
                {
                    self.library.state.trash_origins.insert(trashed.clone(), id);
                    id = trashed;
                }
            }
            let meta = self.library.state.entries.entry(id.clone()).or_default();
            meta.pinned = document.pinned && !document.trashed;
            meta.auto_named = true;
            meta.created_at = document.created_at;
            meta.stamp = Stamp::default();
            renamed.insert(document.id, id);
        }
        if let Some(active) = self.app.active.take() {
            self.app.active = renamed.get(&active).cloned();
        }
        self.library.scan();
    }

    // MARK: - Reading

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
        self.library.list(Collection::All, None, "")
    }

    /// The selected folder or collection, filtered by `query`.
    pub fn list(&self, query: &str) -> Vec<Document> {
        let settings = &self.app.settings;
        self.library
            .list(settings.collection, settings.folder.as_deref(), query)
    }

    pub fn count(&self, collection: Collection) -> usize {
        self.library.count(collection)
    }

    pub fn settings(&self) -> Settings {
        self.app.settings.clone()
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

    /// The library folder on disk.
    pub fn library_root(&self) -> &Path {
        &self.library.root
    }

    pub fn take_conflict(&mut self) -> Option<PathBuf> {
        self.conflict.take()
    }

    /// Whether changed `paths` need a rescan: false when the only change is
    /// Ruwriter's own last write to the open document.
    pub fn needs_rescan(&self, paths: &[PathBuf]) -> bool {
        let active = self.active_document().map(|document| document.path.clone());
        paths
            .iter()
            .any(|path| active.as_ref() != Some(path) || stamp(path) != self.saved_stamp)
    }

    // MARK: - View settings

    pub fn toggle_preview(&mut self) {
        self.options.preview = !self.options.preview;
        self.app.settings.preview = self.options.preview;
        self.persist_app();
    }

    pub fn toggle_focus(&mut self) {
        self.options.focus = !self.options.focus;
    }

    pub fn set_focus(&mut self, focus: bool) {
        self.options.focus = focus;
    }

    pub fn toggle_sidebar(&mut self) {
        self.options.sidebar = !self.options.sidebar;
        self.app.settings.sidebar = self.options.sidebar;
        self.persist_app();
    }

    pub fn set_collection(&mut self, collection: Collection) {
        self.app.settings.collection = collection;
        self.app.settings.folder = None;
        self.persist_app();
    }

    /// Shows `folder`'s documents in the list (`""` is the library folder).
    pub fn set_folder(&mut self, folder: &str) {
        if folder.is_empty() || self.library.folder(folder).is_some() {
            self.app.settings.folder = Some(folder.to_owned());
            self.persist_app();
        }
    }

    pub fn set_expanded(&mut self, folder: &str, expanded: bool) {
        let changed = if expanded {
            self.library.state.expanded.insert(folder.to_owned())
        } else {
            self.library.state.expanded.remove(folder)
        };
        if changed {
            let _ = self.library.save_state();
        }
    }

    pub fn set_appearance(&mut self, appearance: AppearancePreference) {
        self.app.settings.appearance = appearance;
        self.persist_app();
    }

    /// Steps the editor size by `delta` points within the supported range.
    pub fn adjust_font_size(&mut self, delta: i8) {
        let current = i16::from(self.app.settings.font_size);
        let next = (current + i16::from(delta))
            .clamp(i16::from(*FONT_SIZES.start()), i16::from(*FONT_SIZES.end()));
        self.app.settings.font_size = u8::try_from(next).unwrap_or(17);
        self.persist_app();
    }

    pub fn reset_font_size(&mut self) {
        self.app.settings.font_size = Settings::default().font_size;
        self.persist_app();
    }

    // MARK: - Persistence

    fn persist_app(&mut self) {
        self.app.version = APP_STATE_VERSION;
        self.app.active = self.active.clone();
        self.app.external = self.library.external.clone();
        if let Ok(bytes) = serde_json::to_vec_pretty(&self.app)
            && let Ok(json) = String::from_utf8(bytes)
        {
            let _ = write_document(&paths::library_index_path(&self.data_root), &json);
        }
    }

    /// Writes both state files. Failing to persist a cache never blocks work.
    fn persist(&mut self) {
        let _ = self.library.save_state();
        self.persist_app();
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

    /// Explicit save also names a Ruwriter draft after its title and
    /// persists the library state.
    pub fn save(&mut self, text: &str) -> Result<(), String> {
        self.write(text)?;
        self.rename_to_title();
        self.persist();
        Ok(())
    }

    fn write(&mut self, text: &str) -> Result<(), String> {
        let id = self
            .active
            .clone()
            .ok_or_else(|| "没有打开的文档".to_owned())?;
        let path = self.library.path_for(&id);
        if let (Some(known), Some(current)) = (self.saved_stamp, stamp(&path))
            && known != current
        {
            return self.keep_both(&path, text, current);
        }
        write_document(&path, text).map_err(|error| error.to_string())?;
        let written = stamp(&path);
        self.saved = text.to_owned();
        self.saved_stamp = written;
        let stem = file_stem(&path);
        if let Some(meta) = self.library.meta_mut(&id) {
            let derived = derive(text, &stem, meta.auto_named);
            meta.title = derived.title;
            meta.excerpt = derived.excerpt;
            meta.words = derived.words;
            meta.stamp = written.unwrap_or_default();
            let meta = meta.clone();
            if let Some(document) = self.library.documents.iter_mut().find(|d| d.id == id) {
                document.title = meta.title;
                document.excerpt = meta.excerpt;
                document.words = meta.words;
                document.updated_at = now();
            }
        }
        Ok(())
    }

    /// The file changed since Ruwriter read it: the user's text goes to a
    /// copy beside it and the editor takes the version on disk. Nothing is
    /// overwritten.
    fn keep_both(&mut self, path: &Path, text: &str, current: Stamp) -> Result<(), String> {
        let directory = path.parent().unwrap_or(&self.library.root).to_path_buf();
        let copy = unique_path(
            &directory,
            &format!("{} 冲突副本", file_stem(path)),
            DOCUMENT_EXTENSION,
            None,
        );
        write_document(&copy, text).map_err(|error| error.to_string())?;
        self.saved = read_document(path).map_err(|error| error.to_string())?;
        self.saved_stamp = Some(current);
        self.reloads += 1;
        self.conflict = Some(copy);
        self.library.scan();
        Ok(())
    }

    /// Ruwriter drafts follow their title (`标题.md`, `标题 2.md` when taken).
    /// Files named by the user or by other tools keep their names. A failed
    /// rename keeps the old name; the text is already safe.
    fn rename_to_title(&mut self) {
        let Some(id) = self.active.clone() else {
            return;
        };
        let Some(document) = self.library.find(&id).cloned() else {
            return;
        };
        let auto_named = self
            .library
            .state
            .entries
            .get(&id)
            .is_some_and(|meta| meta.auto_named);
        if document.external || document.trashed || !auto_named || document.title == UNTITLED {
            return;
        }
        let directory = parent_directory(&self.library.root, &document.path);
        let target = unique_path(
            &directory,
            &file_name_for(&document.title),
            DOCUMENT_EXTENSION,
            Some(&document.path),
        );
        if target != document.path {
            let _ = self.relocate(&id, &document.path, &target);
        }
    }

    // MARK: - Opening

    fn open_document(&mut self, id: &str) {
        let path = self.library.path_for(id);
        self.saved = read_document(&path).unwrap_or_default();
        self.saved_stamp = stamp(&path);
        self.active = Some(id.to_owned());
    }

    pub fn select(&mut self, id: &str, current: &str) -> Result<String, String> {
        self.autosave(current)?;
        if self.active.as_deref() == Some(id) {
            return Ok(self.saved.clone());
        }
        if self.library.find(id).is_none() {
            return Err("文档不存在".to_owned());
        }
        self.rename_to_title();
        self.open_document(id);
        self.persist();
        Ok(self.saved.clone())
    }

    /// A new blank document in the selected folder, or the library folder.
    pub fn create(&mut self, current: &str) -> Result<String, String> {
        self.autosave(current)?;
        self.rename_to_title();
        let folder = self.app.settings.folder.clone().unwrap_or_default();
        if self.app.settings.folder.is_none()
            && matches!(
                self.app.settings.collection,
                Collection::Trash | Collection::Pinned
            )
        {
            self.app.settings.collection = Collection::All;
        }
        self.activate_new(&folder)?;
        Ok(self.saved.clone())
    }

    fn activate_new(&mut self, folder: &str) -> Result<(), String> {
        let directory = self.folder_path(folder);
        let path = unique_path(&directory, UNTITLED, DOCUMENT_EXTENSION, None);
        write_document(&path, "").map_err(|error| error.to_string())?;
        let id = self
            .library
            .id_for(&path)
            .ok_or_else(|| "无法在资料库中创建文稿".to_owned())?;
        let meta = self.library.state.entries.entry(id.clone()).or_default();
        meta.auto_named = true;
        meta.created_at = now();
        self.library.scan();
        self.open_document(&id);
        self.persist();
        Ok(())
    }

    /// Opens a file. One inside the library is selected (and restored when it
    /// was in the trash); one outside it is listed as an external document and
    /// saved back where it is.
    pub fn open_path(&mut self, path: &Path, current: &str) -> Result<String, String> {
        self.autosave(current)?;
        if let Some(id) = self.library.id_for(path) {
            self.library.scan();
            let mut id = id;
            if self.library.find(&id).is_some_and(|d| d.trashed) {
                id = self.restore(&id)?;
            }
            let saved = self.saved.clone();
            return self.select(&id, &saved);
        }
        if stamp(path).is_none() {
            return Err("无法打开文件".to_owned());
        }
        let id = path.to_string_lossy().into_owned();
        self.library
            .external
            .entry(id.clone())
            .or_insert_with(|| Meta {
                created_at: now(),
                ..Meta::default()
            });
        self.library.scan();
        self.open_document(&id);
        self.persist();
        Ok(self.saved.clone())
    }

    /// Switches to another library folder.
    pub fn open_library(&mut self, folder: &Path, current: &str) -> Result<(), String> {
        self.autosave(current)?;
        self.rename_to_title();
        self.persist();
        std::fs::create_dir_all(folder).map_err(|error| error.to_string())?;
        let default = paths::documents_directory(&self.data_root);
        self.app.library_folder = (folder != default).then(|| folder.to_path_buf());
        self.app.settings.folder = None;
        let external = std::mem::take(&mut self.library.external);
        self.library = Library::open(folder, external);
        let first = self.documents().first().map(|document| document.id.clone());
        match first {
            Some(id) => self.open_document(&id),
            None => self.activate_new("")?,
        }
        self.persist();
        Ok(())
    }

    /// Rereads the library folder. Call after files changed on disk.
    pub fn rescan(&mut self) -> Rescan {
        self.library.scan();
        let Some(id) = self.active.clone() else {
            return Rescan::Unchanged;
        };
        let path = self.library.path_for(&id);
        match stamp(&path) {
            Some(current) if Some(current) != self.saved_stamp => {
                self.saved = read_document(&path).unwrap_or_default();
                self.saved_stamp = Some(current);
                self.reloads += 1;
                Rescan::Reloaded
            }
            Some(_) => Rescan::Unchanged,
            None => {
                // Renamed elsewhere: the same file under another name.
                let known = self.saved_stamp;
                let renamed = known.and_then(|known| {
                    self.library
                        .documents
                        .iter()
                        .find(|document| stamp(&document.path) == Some(known))
                        .map(|document| document.id.clone())
                });
                let outcome = match renamed {
                    Some(new_id) => {
                        self.active = Some(new_id);
                        Rescan::Unchanged
                    }
                    None => {
                        let _ = self.replace_active(None);
                        Rescan::Replaced
                    }
                };
                self.persist_app();
                outcome
            }
        }
    }

    // MARK: - Documents

    pub fn set_pinned(&mut self, id: &str, pinned: bool) -> Result<(), String> {
        let meta = self
            .library
            .meta_mut(id)
            .ok_or_else(|| "文档不存在".to_owned())?;
        meta.pinned = pinned;
        if let Some(document) = self.library.documents.iter_mut().find(|d| d.id == id) {
            document.pinned = pinned;
        }
        self.persist();
        Ok(())
    }

    /// Moves a library document into `folder` (`""` is the library folder).
    pub fn move_document(&mut self, id: &str, folder: &str, current: &str) -> Result<(), String> {
        self.autosave(current)?;
        let document = self.document(id)?;
        if document.external || document.trashed {
            return Err("只能移动资料库中的文稿".to_owned());
        }
        if document.folder.as_deref() == Some(folder) {
            return Ok(());
        }
        let directory = self.folder_path(folder);
        let target = unique_path(&directory, &document.stem(), &extension(&document.path), None);
        self.relocate(id, &document.path, &target)?;
        self.persist();
        Ok(())
    }

    /// Renames a document's file. The name is the user's from now on.
    pub fn rename_document(&mut self, id: &str, name: &str, current: &str) -> Result<(), String> {
        self.autosave(current)?;
        let document = self.document(id)?;
        let name = clean_name(name).ok_or_else(|| "名称无效".to_owned())?;
        let directory = parent_directory(&self.library.root, &document.path);
        let target = unique_path(
            &directory,
            &name,
            &extension(&document.path),
            Some(&document.path),
        );
        let new_id = if target == document.path {
            id.to_owned()
        } else if document.external {
            move_path(&document.path, &target).map_err(|error| error.to_string())?;
            let new_id = target.to_string_lossy().into_owned();
            if let Some(meta) = self.library.external.remove(id) {
                self.library.external.insert(new_id.clone(), meta);
            }
            self.follow(id, &new_id);
            new_id
        } else {
            self.relocate(id, &document.path, &target)?
        };
        if let Some(meta) = self.library.meta_mut(&new_id) {
            meta.auto_named = false;
            // The title may come from the file name: derive it again.
            meta.stamp = Stamp::default();
        }
        self.library.scan();
        self.persist();
        Ok(())
    }

    /// Moves a document to the trash. When it was open, the next document of
    /// the current list opens, or a blank one when nothing is left. External
    /// files only leave the list.
    pub fn trash(&mut self, id: &str, current: &str) -> Result<(), String> {
        self.autosave(current)?;
        let successor = self.successor(id);
        let document = self.document(id)?;
        if document.trashed {
            return Ok(());
        }
        let was_active = self.active.as_deref() == Some(id);
        if document.external {
            self.library.external.remove(id);
            self.library.scan();
        } else {
            let target = unique_path(
                &self.library.trash_path(),
                &document.stem(),
                &extension(&document.path),
                None,
            );
            let new_id = self.relocate(id, &document.path, &target)?;
            self.library
                .state
                .trash_origins
                .insert(new_id.clone(), id.to_owned());
            if let Some(meta) = self.library.state.entries.get_mut(&new_id) {
                meta.pinned = false;
            }
            self.library.scan();
        }
        if was_active {
            self.replace_active(successor)?;
        }
        self.persist();
        Ok(())
    }

    /// Puts a trashed document back where it came from and returns its id.
    pub fn restore(&mut self, id: &str) -> Result<String, String> {
        let document = self.document(id)?;
        if !document.trashed {
            return Ok(id.to_owned());
        }
        let origin = self
            .library
            .state
            .trash_origins
            .get(id)
            .cloned()
            .unwrap_or_else(|| name_of(id).to_owned());
        let directory = self.folder_path(parent_of(&origin));
        let origin_path = PathBuf::from(name_of(&origin));
        let target = unique_path(
            &directory,
            &file_stem(&origin_path),
            &extension(&origin_path),
            None,
        );
        let new_id = self.relocate(id, &document.path, &target)?;
        self.library.state.trash_origins.remove(id);
        self.persist();
        Ok(new_id)
    }

    /// Deletes a trashed document for good.
    pub fn purge(&mut self, id: &str) -> Result<(), String> {
        let successor = self.successor(id);
        let document = self.document(id)?;
        if !document.trashed {
            return Err("只能删除废纸篓中的文档".to_owned());
        }
        remove_file(&document.path)?;
        self.library.scan();
        if self.active.as_deref() == Some(id) {
            self.replace_active(successor)?;
        }
        self.persist();
        Ok(())
    }

    pub fn empty_trash(&mut self) -> Result<(), String> {
        let trash = self.library.trash_path();
        let active_trashed = self.active_document().is_some_and(|d| d.trashed);
        match std::fs::remove_dir_all(&trash) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
        self.library.state.trash_origins.clear();
        self.library.scan();
        if active_trashed {
            self.replace_active(None)?;
        }
        self.persist();
        Ok(())
    }

    // MARK: - Folders

    /// Creates a folder inside `parent` (`""` is the library folder) and
    /// returns its id.
    pub fn create_folder(&mut self, parent: &str, name: &str) -> Result<String, String> {
        if !parent.is_empty() && self.library.folder(parent).is_none() {
            return Err("文件夹不存在".to_owned());
        }
        let name = clean_name(name).unwrap_or_else(|| NEW_FOLDER.to_owned());
        let target = unique_path(&self.folder_path(parent), &name, "", None);
        std::fs::create_dir_all(&target).map_err(|error| error.to_string())?;
        let id = self
            .library
            .id_for(&target)
            .ok_or_else(|| "无法创建文件夹".to_owned())?;
        if !parent.is_empty() {
            self.library.state.expanded.insert(parent.to_owned());
        }
        self.library.scan();
        self.persist();
        Ok(id)
    }

    pub fn rename_folder(&mut self, id: &str, name: &str, current: &str) -> Result<String, String> {
        self.autosave(current)?;
        if self.library.folder(id).is_none() {
            return Err("文件夹不存在".to_owned());
        }
        let name = clean_name(name).ok_or_else(|| "名称无效".to_owned())?;
        let from = self.library.path_for(id);
        let parent = parent_of(id).to_owned();
        let target = unique_path(&self.folder_path(&parent), &name, "", Some(&from));
        if target == from {
            return Ok(id.to_owned());
        }
        let new_id = self.relocate(id, &from, &target)?;
        let old_name = name_of(id).to_owned();
        let new_name = name_of(&new_id).to_owned();
        if let Some(names) = self.library.state.order.get_mut(&parent) {
            for existing in names.iter_mut() {
                if *existing == old_name {
                    existing.clone_from(&new_name);
                }
            }
        }
        self.library.scan();
        self.persist();
        Ok(new_id)
    }

    /// Moves a folder into `parent`, placed before the sibling named
    /// `before` (at the end without one). Within the same parent it only
    /// rearranges.
    pub fn move_folder(
        &mut self,
        id: &str,
        parent: &str,
        before: Option<&str>,
        current: &str,
    ) -> Result<String, String> {
        self.autosave(current)?;
        if self.library.folder(id).is_none() {
            return Err("文件夹不存在".to_owned());
        }
        if !parent.is_empty() && Library::is_within(parent, id) {
            return Err("不能把文件夹移到它自己里面".to_owned());
        }
        let new_id = if parent_of(id) == parent {
            id.to_owned()
        } else {
            let from = self.library.path_for(id);
            let target = unique_path(&self.folder_path(parent), name_of(id), "", None);
            self.relocate(id, &from, &target)?
        };
        let name = name_of(&new_id).to_owned();
        self.library.arrange(parent, &name, before);
        self.library.scan();
        self.persist();
        Ok(new_id)
    }

    /// Moves a folder and everything in it to the trash; each document can be
    /// restored to where it was.
    pub fn trash_folder(&mut self, id: &str, current: &str) -> Result<(), String> {
        self.autosave(current)?;
        if self.library.folder(id).is_none() {
            return Err("文件夹不存在".to_owned());
        }
        let inside: Vec<String> = self
            .library
            .documents
            .iter()
            .filter(|document| !document.trashed && !document.external)
            .filter(|document| Library::is_within(&document.id, id))
            .map(|document| document.id.clone())
            .collect();
        let active_inside = self
            .active
            .as_deref()
            .is_some_and(|active| inside.iter().any(|inner| inner == active));
        let from = self.library.path_for(id);
        let target = unique_path(&self.library.trash_path(), name_of(id), "", None);
        let trashed = self.relocate(id, &from, &target)?;
        for old in inside {
            let rest = old.strip_prefix(id).unwrap_or_default();
            self.library
                .state
                .trash_origins
                .insert(format!("{trashed}{rest}"), old);
        }
        if self
            .app
            .settings
            .folder
            .as_deref()
            .is_some_and(|folder| Library::is_within(folder, &trashed))
        {
            self.app.settings.folder = None;
        }
        self.library.scan();
        if active_inside {
            self.replace_active(None)?;
        }
        self.persist();
        Ok(())
    }

    // MARK: - Helpers

    fn document(&self, id: &str) -> Result<Document, String> {
        self.library
            .find(id)
            .cloned()
            .ok_or_else(|| "文档不存在".to_owned())
    }

    fn folder_path(&self, folder: &str) -> PathBuf {
        if folder.is_empty() {
            self.library.root.clone()
        } else {
            self.library.path_for(folder)
        }
    }

    /// Moves a file or folder inside the library and carries its cached
    /// state, the open document and the selected folder along. Returns the
    /// new id.
    fn relocate(&mut self, id: &str, from: &Path, to: &Path) -> Result<String, String> {
        move_path(from, to).map_err(|error| error.to_string())?;
        let new_id = self
            .library
            .id_for(to)
            .ok_or_else(|| "目标不在资料库中".to_owned())?;
        self.library.rekey(id, &new_id);
        self.follow(id, &new_id);
        self.library.scan();
        Ok(new_id)
    }

    /// Rewrites ids under `from` that the session remembers.
    fn follow(&mut self, from: &str, to: &str) {
        let rewrite = |value: &str| -> Option<String> {
            if value == from {
                Some(to.to_owned())
            } else {
                value
                    .strip_prefix(&format!("{from}/"))
                    .map(|rest| format!("{to}/{rest}"))
            }
        };
        if let Some(active) = self.active.as_deref().and_then(rewrite) {
            self.active = Some(active);
        }
        if let Some(folder) = self.app.settings.folder.as_deref().and_then(rewrite) {
            self.app.settings.folder = Some(folder);
        }
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
                self.open_document(&id);
                Ok(())
            }
            None => self.activate_new(""),
        }
    }
}

fn parent_directory(root: &Path, path: &Path) -> PathBuf {
    path.parent().unwrap_or(root).to_path_buf()
}

fn extension(path: &Path) -> String {
    path.extension()
        .map(|extension| extension.to_string_lossy().into_owned())
        .unwrap_or_else(|| DOCUMENT_EXTENSION.to_owned())
}

/// A file or folder name the user typed, without separators or a leading
/// dot (which would hide it). `None` when nothing usable is left.
fn clean_name(name: &str) -> Option<String> {
    let cleaned: String = name
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            control if control.is_control() => ' ',
            other => other,
        })
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').trim();
    let cleaned: String = cleaned.chars().take(120).collect();
    (!cleaned.is_empty()).then_some(cleaned)
}

fn remove_file(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}
