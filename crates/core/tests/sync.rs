//! Three-way merge rules for Gist sync, independent of the network.

use std::collections::BTreeMap;

use ruwriter_core::sync::{SyncBase, SyncDocument, content_hash, gist_name, plan_sync, uid_for};

fn doc(uid: &str, path: &str, text: &str) -> SyncDocument {
    SyncDocument {
        uid: uid.to_owned(),
        path: path.to_owned(),
        content: text.to_owned(),
        hash: content_hash(text),
        updated_at: 10,
        deleted: false,
        title: path.trim_end_matches(".txt").to_owned(),
        synopsis: String::new(),
        status: Default::default(),
        tags: Vec::new(),
        pinned: false,
        created_at: 1,
    }
}

fn deleted(uid: &str, path: &str) -> SyncDocument {
    SyncDocument {
        uid: uid.to_owned(),
        path: path.to_owned(),
        content: String::new(),
        hash: String::new(),
        updated_at: 0,
        deleted: true,
        title: String::new(),
        synopsis: String::new(),
        status: Default::default(),
        tags: Vec::new(),
        pinned: false,
        created_at: 0,
    }
}

fn base(uid: &str, path: &str, text: &str) -> (String, SyncBase) {
    (
        uid.to_owned(),
        SyncBase {
            path: path.to_owned(),
            hash: content_hash(text),
            updated_at: 10,
        },
    )
}

#[test]
fn ids_are_stable_and_gist_names_are_flat() {
    assert_eq!(uid_for("第一章/雨夜.txt", 1), uid_for("第一章/雨夜.txt", 1));
    assert_ne!(uid_for("第一章/雨夜.txt", 1), uid_for("第二章/雨夜.txt", 1));
    let name = gist_name("abc123");
    assert_eq!(name, "doc_abc123.txt");
    assert!(!name.contains('/'));
}

#[test]
fn first_sync_uploads_every_active_document() {
    let local = vec![
        doc("a", "第一章.txt", "雨夜"),
        doc("b", "第二章.txt", "清晨"),
    ];
    let plan = plan_sync(&local, &[], &BTreeMap::new(), "lib", "mac", 100);
    assert_eq!(plan.uploads.len(), 2);
    assert_eq!(plan.downloads.len(), 0);
    assert_eq!(plan.manifest.documents.len(), 2);
    assert!(plan.manifest.documents.iter().all(|d| !d.deleted));
}

#[test]
fn unchanged_remote_does_not_reupload() {
    let local = vec![doc("a", "第一章.txt", "雨夜")];
    let remote = vec![doc("a", "第一章.txt", "雨夜")];
    let base = BTreeMap::from([base("a", "第一章.txt", "雨夜")]);
    let plan = plan_sync(&local, &remote, &base, "lib", "mac", 100);
    assert!(plan.uploads.is_empty());
    assert!(plan.downloads.is_empty());
    assert!(plan.conflicts.is_empty());
}

#[test]
fn remote_edit_is_pulled_when_local_is_unchanged() {
    let local = vec![doc("a", "第一章.txt", "雨夜")];
    let remote = vec![doc("a", "第一章.txt", "雨夜，风更大")];
    let base = BTreeMap::from([base("a", "第一章.txt", "雨夜")]);
    let plan = plan_sync(&local, &remote, &base, "lib", "mac", 100);
    assert_eq!(plan.downloads.len(), 1);
    assert!(plan.uploads.is_empty());
    assert_eq!(plan.downloads[0].content, "雨夜，风更大");
}

#[test]
fn local_edit_is_uploaded_when_remote_is_unchanged() {
    let local = vec![doc("a", "第一章.txt", "雨夜，风更大")];
    let remote = vec![doc("a", "第一章.txt", "雨夜")];
    let base = BTreeMap::from([base("a", "第一章.txt", "雨夜")]);
    let plan = plan_sync(&local, &remote, &base, "lib", "mac", 100);
    assert_eq!(plan.uploads.len(), 1);
    assert!(plan.downloads.is_empty());
}

#[test]
fn both_edits_keep_local_and_return_remote_as_conflict() {
    let local = vec![doc("a", "第一章.txt", "本地改过")];
    let remote = vec![doc("a", "第一章.txt", "云端改过")];
    let base = BTreeMap::from([base("a", "第一章.txt", "原稿")]);
    let plan = plan_sync(&local, &remote, &base, "lib", "mac", 100);
    assert_eq!(plan.uploads.len(), 1);
    assert_eq!(plan.conflicts.len(), 1);
    assert_eq!(plan.conflicts[0].content, "云端改过");
    assert_eq!(plan.uploads[0].content, "本地改过");
}

#[test]
fn remote_delete_moves_local_to_trash_when_local_is_unchanged() {
    let local = vec![doc("a", "第一章.txt", "雨夜")];
    let remote = vec![deleted("a", "第一章.txt")];
    let base = BTreeMap::from([base("a", "第一章.txt", "雨夜")]);
    let plan = plan_sync(&local, &remote, &base, "lib", "mac", 100);
    assert_eq!(plan.local_deletes, vec!["a"]);
    assert!(
        plan.manifest
            .documents
            .iter()
            .any(|d| d.uid == "a" && d.deleted)
    );
}

#[test]
fn local_delete_stays_deleted_on_remote() {
    let local = vec![deleted("a", "第一章.txt")];
    let remote = vec![doc("a", "第一章.txt", "雨夜")];
    let base = BTreeMap::from([base("a", "第一章.txt", "雨夜")]);
    let plan = plan_sync(&local, &remote, &base, "lib", "mac", 100);
    assert!(plan.uploads.is_empty());
    assert!(
        plan.manifest
            .documents
            .iter()
            .any(|d| d.uid == "a" && d.deleted)
    );
}

#[test]
fn remote_new_document_is_imported() {
    let local = Vec::new();
    let remote = vec![doc("a", "第一章.txt", "雨夜")];
    let plan = plan_sync(&local, &remote, &BTreeMap::new(), "lib", "mac", 100);
    assert_eq!(plan.downloads.len(), 1);
    assert_eq!(plan.downloads[0].path, "第一章.txt");
    assert_eq!(plan.manifest.documents.len(), 1);
}

#[test]
fn remote_tombstone_is_preserved_for_a_third_device() {
    let local = Vec::new();
    let remote = vec![deleted("a", "第一章.txt")];
    let plan = plan_sync(&local, &remote, &BTreeMap::new(), "lib", "mac", 100);
    assert!(plan.downloads.is_empty());
    assert!(
        plan.manifest
            .documents
            .iter()
            .any(|d| d.uid == "a" && d.deleted)
    );
}

#[test]
fn local_edit_after_remote_delete_resurrects() {
    let local = vec![doc("a", "第一章.txt", "本地又写了")];
    let remote = vec![deleted("a", "第一章.txt")];
    let base = BTreeMap::from([base("a", "第一章.txt", "旧稿")]);
    let plan = plan_sync(&local, &remote, &base, "lib", "mac", 100);
    assert!(plan.local_deletes.is_empty());
    assert_eq!(plan.uploads.len(), 1);
    assert!(
        plan.manifest
            .documents
            .iter()
            .any(|d| d.uid == "a" && !d.deleted)
    );
}

#[test]
fn library_id_and_device_are_stamped() {
    let local = vec![doc("a", "第一章.txt", "雨夜")];
    let plan = plan_sync(&local, &[], &BTreeMap::new(), "lib-1", "mac", 123);
    assert_eq!(plan.manifest.library_id, "lib-1");
    assert_eq!(plan.manifest.device, "mac");
    assert_eq!(plan.manifest.updated_at, 123);
    assert_eq!(plan.manifest.schema, 1);
}
