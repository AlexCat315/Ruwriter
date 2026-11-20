//! Cloud backup for the library through a private GitHub Gist.
//!
//! Plain text stays the truth on disk. The Gist holds one flat file per
//! document (Gist rejects names containing `/`) plus a small manifest that
//! records the library id, paths and content hashes. A three-way comparison
//! between local files, the last-synced base and the remote manifest decides
//! what to upload, download, delete or keep as a conflict copy.

use crate::document::DocStatus;
use crate::files::{read_document, write_document};
use crate::session::Session;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub const SCHEMA: u32 = 1;
pub const MANIFEST_FILE: &str = "ruwriter-manifest.json";

/// One document as it exists locally at the moment sync starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncDocument {
    pub uid: String,
    /// Library-relative path with `/` separators. Trashed documents keep the
    /// path they had before they were trashed.
    pub path: String,
    pub content: String,
    pub hash: String,
    pub updated_at: u64,
    pub deleted: bool,
    pub title: String,
    pub synopsis: String,
    pub status: DocStatus,
    pub tags: Vec<String>,
    pub pinned: bool,
    pub created_at: u64,
}

impl SyncDocument {
    pub fn gist_name(&self) -> String {
        gist_name(&self.uid)
    }

    fn manifest(&self) -> ManifestDoc {
        ManifestDoc {
            uid: self.uid.clone(),
            path: self.path.clone(),
            file: self.gist_name(),
            hash: self.hash.clone(),
            updated_at: self.updated_at,
            deleted: self.deleted,
            title: self.title.clone(),
            synopsis: self.synopsis.clone(),
            status: self.status,
            tags: self.tags.clone(),
            pinned: self.pinned,
            created_at: self.created_at,
        }
    }
}

/// The document entry stored in the remote manifest.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ManifestDoc {
    pub uid: String,
    pub path: String,
    pub file: String,
    pub hash: String,
    pub updated_at: u64,
    pub deleted: bool,
    pub title: String,
    pub synopsis: String,
    pub status: DocStatus,
    pub tags: Vec<String>,
    pub pinned: bool,
    pub created_at: u64,
}

/// The small JSON file at the root of the Gist.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Manifest {
    pub schema: u32,
    pub library_id: String,
    pub updated_at: u64,
    pub device: String,
    pub documents: Vec<ManifestDoc>,
}

/// What this device last saw in the Gist, per document uid.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncBase {
    pub path: String,
    pub hash: String,
    pub updated_at: u64,
}

/// Sync settings stored in `library.json`, so they follow the library.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SyncConfig {
    pub gist_id: Option<String>,
    pub library_id: String,
    pub last_synced_at: u64,
    pub base: BTreeMap<String, SyncBase>,
}

/// The changes one sync pass should make.
#[derive(Clone, Debug, Default)]
pub struct SyncPlan {
    pub manifest: Manifest,
    pub uploads: Vec<SyncDocument>,
    pub downloads: Vec<SyncDocument>,
    pub local_deletes: Vec<String>,
    pub conflicts: Vec<SyncDocument>,
    pub summary: String,
}

/// Result handed back to the UI thread after the network work finishes.
#[derive(Clone, Debug)]
pub struct SyncReport {
    pub gist_id: String,
    pub created: bool,
    pub manifest: Manifest,
    pub pulled: Vec<SyncDocument>,
    pub deleted: Vec<String>,
    pub conflicts: Vec<SyncDocument>,
    pub summary: String,
    pub base: BTreeMap<String, SyncBase>,
}

#[derive(Clone, Debug, Default)]
pub struct SyncOutcome {
    pub active_changed: bool,
    pub conflicts: Vec<PathBuf>,
    pub summary: String,
}

impl Session {
    pub fn sync_config(&self) -> &SyncConfig {
        &self.app.sync
    }

    pub fn set_sync_config(&mut self, config: SyncConfig) {
        self.app.sync = config;
        self.persist_app();
    }

    /// Makes sure every library document has a stable sync uid. Uids follow
    /// the cached meta, so renaming or moving a file keeps its identity.
    pub fn ensure_sync_uids(&mut self) {
        let entries = &mut self.library.state.entries;
        for (id, meta) in entries.iter_mut() {
            if meta.uid.is_empty() {
                meta.uid = uid_for(id, meta.created_at);
            }
        }
    }

    /// Reads the local library into a form the sync planner can compare.
    /// Reads every file, so call this off the UI thread when the library is
    /// large; the UI only snapshots the paths and metadata.
    pub fn sync_snapshot(&mut self) -> Vec<SyncDocument> {
        self.ensure_sync_uids();
        let mut snapshot = Vec::new();
        for document in self.library.documents.iter().filter(|d| !d.external) {
            let Some(meta) = self.library.meta(&document.id) else {
                continue;
            };
            let uid = if meta.uid.is_empty() {
                uid_for(&document.id, meta.created_at)
            } else {
                meta.uid.clone()
            };
            let path = if document.trashed {
                self.library
                    .state
                    .trash_origins
                    .get(&document.id)
                    .cloned()
                    .unwrap_or_else(|| document.id.clone())
            } else {
                document.id.clone()
            };
            let content = if document.trashed {
                String::new()
            } else {
                read_document(&document.path).unwrap_or_default()
            };
            snapshot.push(SyncDocument {
                uid,
                path,
                hash: if document.trashed {
                    String::new()
                } else {
                    content_hash(&content)
                },
                updated_at: document.updated_at,
                deleted: document.trashed,
                title: meta.title.clone(),
                synopsis: meta.synopsis.clone(),
                status: meta.status,
                tags: meta.tags.clone(),
                pinned: meta.pinned,
                created_at: meta.created_at,
                content,
            });
        }
        snapshot
    }

    /// Applies the planner's decisions on the UI thread. Pulls are written
    /// through the normal document writer; remote deletions go to the trash
    /// rather than disappearing.
    pub fn apply_sync_report(&mut self, report: &SyncReport) -> Result<SyncOutcome, String> {
        let mut outcome = SyncOutcome {
            summary: report.summary.clone(),
            ..SyncOutcome::default()
        };

        for doc in &report.pulled {
            if doc.deleted {
                continue;
            }
            match self.id_for_uid(&doc.uid) {
                Some(id) if !self.library.find(&id).is_some_and(|d| d.trashed) => {
                    let path = self.library.path_for(&id);
                    write_document(&path, &doc.content).map_err(|e| e.to_string())?;
                    if let Some(meta) = self.library.meta_mut(&id) {
                        meta.uid = doc.uid.clone();
                        meta.title = doc.title.clone();
                        meta.synopsis = doc.synopsis.clone();
                        meta.status = doc.status;
                        meta.tags = doc.tags.clone();
                        meta.pinned = doc.pinned;
                        if meta.created_at == 0 {
                            meta.created_at = doc.created_at;
                        }
                    }
                    if self.active.as_deref() == Some(&id) {
                        self.saved = doc.content.clone();
                        self.saved_stamp = crate::files::stamp(&path);
                        outcome.active_changed = true;
                    }
                }
                Some(_) => {
                    // A remote active copy meets a local trashed copy: leave
                    // the trash alone. The planner normally turns this into a
                    // remote tombstone instead.
                }
                None => {
                    let id = self.import_sync_document(doc)?;
                    if self.active.is_none() {
                        self.open_document(&id);
                    }
                }
            }
        }

        for uid in &report.deleted {
            let Some(id) = self.id_for_uid(uid) else {
                continue;
            };
            if self.library.find(&id).is_some_and(|d| d.trashed) {
                continue;
            }
            if self.active.as_deref() == Some(&id) {
                outcome.active_changed = true;
            }
            let current = self.saved.clone();
            self.trash(&id, &current)?;
        }

        for doc in &report.conflicts {
            if let Some(path) = self.write_conflict_copy(doc)? {
                outcome.conflicts.push(path);
            }
        }

        self.app.sync.gist_id = Some(report.gist_id.clone());
        self.app.sync.last_synced_at = now();
        self.app.sync.base = report.base.clone();
        self.library.scan();
        if self.active.is_none() {
            let first = self.documents().first().map(|d| d.id.clone());
            match first {
                Some(id) => self.open_document(&id),
                None => self.activate_new("")?,
            }
        }
        self.persist();
        Ok(outcome)
    }

    fn id_for_uid(&self, uid: &str) -> Option<String> {
        self.library
            .state
            .entries
            .iter()
            .find(|(_, meta)| meta.uid == uid)
            .map(|(id, _)| id.clone())
    }

    fn import_sync_document(&mut self, doc: &SyncDocument) -> Result<String, String> {
        let relative = safe_relative(&doc.path).unwrap_or_else(|| format!("{}.txt", doc.uid));
        let path = self.library.root.join(&relative);
        let directory = path.parent().unwrap_or(&self.library.root).to_path_buf();
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| doc.title.clone());
        let extension = path
            .extension()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "txt".to_owned());
        std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        let target = crate::files::unique_path(&directory, &stem, &extension, None);
        write_document(&target, &doc.content).map_err(|e| e.to_string())?;
        let id = self
            .library
            .id_for(&target)
            .ok_or_else(|| "无法在资料库中创建文稿".to_owned())?;
        let meta = self.library.state.entries.entry(id.clone()).or_default();
        meta.uid = doc.uid.clone();
        meta.title = doc.title.clone();
        meta.synopsis = doc.synopsis.clone();
        meta.status = doc.status;
        meta.tags = doc.tags.clone();
        meta.pinned = doc.pinned;
        meta.created_at = doc.created_at;
        self.library.scan();
        Ok(id)
    }

    fn write_conflict_copy(&mut self, doc: &SyncDocument) -> Result<Option<PathBuf>, String> {
        let relative = safe_relative(&doc.path).unwrap_or_else(|| format!("{}.txt", doc.uid));
        let path = self.library.root.join(relative);
        let directory = path.parent().unwrap_or(&self.library.root).to_path_buf();
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| doc.title.clone());
        let extension = path
            .extension()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "txt".to_owned());
        let stamp = local_timestamp();
        let name = format!("{stem}（云端冲突 {stamp}）");
        let target = crate::files::unique_path(&directory, &name, &extension, None);
        write_document(&target, &doc.content).map_err(|e| e.to_string())?;
        self.library.scan();
        Ok(Some(target))
    }
}

/// Three-way merge of local, last-synced and remote documents.
pub fn plan_sync(
    local: &[SyncDocument],
    remote: &[SyncDocument],
    base: &BTreeMap<String, SyncBase>,
    library_id: &str,
    device: &str,
    now: u64,
) -> SyncPlan {
    let local: BTreeMap<&str, &SyncDocument> = local
        .iter()
        .filter(|d| !d.uid.is_empty())
        .map(|d| (d.uid.as_str(), d))
        .collect();
    let remote: BTreeMap<&str, &SyncDocument> = remote
        .iter()
        .filter(|d| !d.uid.is_empty())
        .map(|d| (d.uid.as_str(), d))
        .collect();
    let mut uids: BTreeSet<&str> = local.keys().copied().collect();
    uids.extend(remote.keys().copied());
    uids.extend(base.keys().map(String::as_str));
    let mut plan = SyncPlan::default();
    let mut uploads = 0;
    let mut downloads = 0;
    let mut deletes = 0;
    let mut conflicts = 0;

    for uid in uids {
        let l = local.get(uid).copied();
        let r = remote.get(uid).copied();
        let b = base.get(uid);
        let l_active = l.filter(|d| !d.deleted);
        let r_active = r.filter(|d| !d.deleted);

        if let Some(la) = l_active {
            if let Some(ra) = r_active {
                let result = if la.hash == ra.hash {
                    la
                } else if b.is_some_and(|b| b.hash == la.hash) {
                    downloads += 1;
                    plan.downloads.push(ra.clone());
                    ra
                } else if b.is_some_and(|b| b.hash == ra.hash) {
                    uploads += 1;
                    plan.uploads.push(la.clone());
                    la
                } else {
                    // Both sides changed. Keep the local text as the
                    // canonical copy and hand the remote text back as a
                    // conflict copy; nothing is discarded.
                    conflicts += 1;
                    plan.conflicts.push(ra.clone());
                    uploads += 1;
                    plan.uploads.push(la.clone());
                    la
                };
                push_manifest(&mut plan, result, now);
                continue;
            }

            let remote_deleted = r.is_some_and(|d| d.deleted);
            let should_delete = b.is_some_and(|b| b.hash == la.hash);
            if remote_deleted && should_delete || r.is_none() && b.is_some() && should_delete {
                deletes += 1;
                plan.local_deletes.push(uid.to_owned());
                push_tombstone(&mut plan, uid, b, r, l, now);
            } else {
                // Local edit after a remote delete wins and resurrects the
                // document.
                uploads += 1;
                plan.uploads.push(la.clone());
                push_manifest(&mut plan, la, now);
            }
            continue;
        }

        // Local is deleted (trashed) or absent.
        if let Some(ra) = r_active {
            if b.is_some() {
                // Local deletion wins; keep the tombstone in the manifest.
                push_tombstone(&mut plan, uid, b, r, l, now);
            } else {
                downloads += 1;
                plan.downloads.push(ra.clone());
                push_manifest(&mut plan, ra, now);
            }
            continue;
        }

        push_tombstone(&mut plan, uid, b, r, l, now);
    }

    plan.manifest = Manifest {
        schema: SCHEMA,
        library_id: library_id.to_owned(),
        updated_at: now,
        device: device.to_owned(),
        documents: plan.manifest.documents,
    };
    plan.summary = format!(
        "上传 {uploads} 篇，下载 {downloads} 篇，云端删除 {deletes} 篇，冲突 {conflicts} 篇"
    );
    plan
}

fn push_manifest(plan: &mut SyncPlan, doc: &SyncDocument, _now: u64) {
    plan.manifest.documents.push(doc.manifest());
}

fn push_tombstone(
    plan: &mut SyncPlan,
    uid: &str,
    base: Option<&SyncBase>,
    remote: Option<&SyncDocument>,
    local: Option<&SyncDocument>,
    _now: u64,
) {
    let path = local
        .map(|d| d.path.clone())
        .or_else(|| remote.map(|d| d.path.clone()))
        .or_else(|| base.map(|b| b.path.clone()))
        .unwrap_or_else(|| format!("{uid}.txt"));
    let updated_at = remote.map(|d| d.updated_at).unwrap_or_else(|| now());
    plan.manifest.documents.push(ManifestDoc {
        uid: uid.to_owned(),
        path,
        file: gist_name(uid),
        hash: String::new(),
        updated_at,
        deleted: true,
        ..ManifestDoc::default()
    });
}

pub fn remote_documents(
    manifest: &Manifest,
    contents: &BTreeMap<String, String>,
) -> Vec<SyncDocument> {
    manifest
        .documents
        .iter()
        .filter(|d| !d.deleted)
        .filter_map(|d| {
            let content = contents.get(&d.file).cloned()?;
            Some(SyncDocument {
                uid: d.uid.clone(),
                path: d.path.clone(),
                hash: content_hash(&content),
                content,
                updated_at: d.updated_at,
                deleted: false,
                title: d.title.clone(),
                synopsis: d.synopsis.clone(),
                status: d.status,
                tags: d.tags.clone(),
                pinned: d.pinned,
                created_at: d.created_at,
            })
        })
        .collect()
}

pub fn base_for(manifest: &Manifest) -> BTreeMap<String, SyncBase> {
    manifest
        .documents
        .iter()
        .filter(|d| !d.deleted)
        .map(|d| {
            (
                d.uid.clone(),
                SyncBase {
                    path: d.path.clone(),
                    hash: d.hash.clone(),
                    updated_at: d.updated_at,
                },
            )
        })
        .collect()
}

pub fn content_hash(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

pub fn gist_name(uid: &str) -> String {
    format!("doc_{uid}.txt")
}

/// Stable per-document identity. Renaming keeps the cached meta, so the uid
/// survives a rename; two copies of a synced library share it.
pub fn uid_for(id: &str, created_at: u64) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    let mut feed = |bytes: &[u8]| {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    };
    feed(id.as_bytes());
    feed(b"\0");
    feed(&created_at.to_le_bytes());
    format!("{hash:016x}")
}

pub fn new_library_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let pid = u128::from(std::process::id());
    let value = nanos ^ (pid << 64);
    format!("{:024x}", value as u64)
}

pub fn default_device() -> String {
    std::env::var("RUWRITER_DEVICE")
        .ok()
        .or_else(|| std::env::var("HOSTNAME").ok())
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .unwrap_or_else(|| std::env::consts::OS.to_owned())
}

fn safe_relative(path: &str) -> Option<String> {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed.starts_with('/') || trimmed.contains('\0') {
        return None;
    }
    let parts: Vec<&str> = trimmed.split('/').filter(|p| !p.is_empty()).collect();
    if parts
        .iter()
        .any(|p| *p == ".." || *p == "." || p.starts_with('.'))
    {
        return None;
    }
    Some(parts.join("/"))
}

fn local_timestamp() -> String {
    // The UI already has a date formatter; this is only a short, sortable
    // suffix for a conflict file name.
    let secs = now();
    let days = secs / 86_400;
    let time = secs % 86_400;
    let (year, month, day) = civil_from_days(days as i64);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}{:02}",
        time / 3600,
        (time % 3600) / 60
    )
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    ((y + (m <= 2) as i64) as i32, m as u32, d as u32)
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

/// Ensures a new library has an id before the first sync.
pub fn ensure_library_id(session: &mut Session) {
    if session.sync_config().library_id.is_empty() {
        let mut config = session.sync_config().clone();
        config.library_id = new_library_id();
        session.set_sync_config(config);
    }
}
