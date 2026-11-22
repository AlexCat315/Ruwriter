//! Three-way merge rules for Gist sync, independent of the network.

use std::collections::BTreeMap;
use std::fs;

use ruwriter_core::Session;
use ruwriter_core::sync::{
    SyncBase, SyncDocument, SyncPlan, SyncReport, base_for, content_hash, gist_name, plan_sync,
    uid_for,
};

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
            metadata: None,
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
fn missing_remote_manifest_entry_reuploads_the_local_document() {
    let local = vec![doc("a", "第一章.txt", "雨夜")];
    let base = BTreeMap::from([base("a", "第一章.txt", "雨夜")]);
    let plan = plan_sync(&local, &[], &base, "lib", "mac", 100);
    assert!(plan.local_deletes.is_empty());
    assert_eq!(plan.uploads, local);
}

#[test]
fn a_local_file_absent_without_delete_intent_is_recovered_from_the_cloud() {
    let remote = vec![doc("a", "第一章.txt", "雨夜")];
    let base = BTreeMap::from([base("a", "第一章.txt", "雨夜")]);
    let plan = plan_sync(&[], &remote, &base, "lib", "mac", 100);
    assert_eq!(plan.downloads, remote);
    assert!(!plan.manifest.documents[0].deleted);
    assert!(
        plan_sync(&[], &[], &base, "lib", "mac", 100)
            .manifest
            .documents
            .is_empty()
    );
}

#[test]
fn purging_or_emptying_trash_keeps_an_explicit_deletion_across_restart() {
    for empty in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut session = Session::at(dir.path());
        session.autosave("雨夜").unwrap();
        let remote = session.sync_snapshot().unwrap();
        let document = &remote[0];
        session.trash(&document.path, "雨夜").unwrap();
        if empty {
            session.empty_trash().unwrap();
        } else {
            let trashed = session
                .library
                .documents
                .iter()
                .find(|doc| doc.trashed)
                .unwrap()
                .id
                .clone();
            session.purge(&trashed).unwrap();
        }
        let mut reopened = Session::at(dir.path());
        let snapshot = reopened.sync_snapshot().unwrap();
        let plan = plan_sync(&snapshot, &remote, &BTreeMap::new(), "lib", "mac", 100);
        assert!(plan.downloads.is_empty());
        assert!(
            plan.manifest
                .documents
                .iter()
                .any(|entry| entry.uid == document.uid && entry.deleted)
        );
    }
}

#[test]
fn versioned_tombstone_only_removes_the_matching_version() {
    let version = doc("a", "第一章.txt", "雨夜");
    let remote = vec![SyncDocument {
        deleted: true,
        content: String::new(),
        ..version.clone()
    }];
    let base = BTreeMap::from([base("a", "第一章.txt", "较早的底稿")]);
    let matched = plan_sync(
        std::slice::from_ref(&version),
        &remote,
        &base,
        "lib",
        "mac",
        100,
    );
    assert_eq!(matched.local_deletes, ["a"]);
    assert_eq!(matched.manifest.documents[0].hash, version.hash);
    let local = vec![doc("a", "第一章.txt", "未经同步的改稿")];
    let edited = plan_sync(&local, &remote, &BTreeMap::new(), "lib", "mac", 100);
    assert!(edited.local_deletes.is_empty());
    assert_eq!(edited.uploads, local);
}

#[test]
fn local_delete_requires_the_previewed_disk_version() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::at(dir.path());
    session.autosave("旧稿").unwrap();
    let old = session.sync_snapshot().unwrap().remove(0);
    let path = session.library_root().join(&old.path);
    fs::write(&path, "另外设备改过的版本").unwrap();
    assert!(session.trash_sync_document(&old, "旧稿").is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "另外设备改过的版本");
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

fn report(plan: SyncPlan) -> SyncReport {
    SyncReport {
        gist_id: "test-gist".to_owned(),
        created: false,
        base: base_for(&plan.manifest),
        manifest: plan.manifest,
        pulled: plan.downloads,
        deleted: plan.local_deletes,
        conflicts: plan.conflicts,
        summary: plan.summary,
    }
}

#[test]
fn legacy_sync_config_migrates_and_follows_its_library() {
    let data = tempfile::tempdir().unwrap();
    fs::write(
        data.path().join("library.json"),
        r#"{"sync":{"library_id":"old-library","gist_id":"old-gist","last_synced_at":123}}"#,
    )
    .unwrap();
    let mut session = Session::at(data.path());
    assert_eq!(session.sync_config().library_id, "old-library");
    assert_eq!(session.sync_config().gist_id.as_deref(), Some("old-gist"));

    let other = tempfile::tempdir().unwrap();
    session.open_library(other.path(), "").unwrap();
    let other_id = session.sync_config().library_id.clone();
    assert_ne!(other_id, "old-library");
    assert!(session.sync_config().gist_id.is_none());
    assert_eq!(Session::at(data.path()).sync_config().library_id, other_id);

    session
        .open_library(&data.path().join("documents"), "")
        .unwrap();
    assert_eq!(session.sync_config().library_id, "old-library");
    assert_eq!(session.sync_config().last_synced_at, 123);
    assert_eq!(
        Session::at(data.path()).sync_config().gist_id.as_deref(),
        Some("old-gist")
    );
}

#[test]
fn pull_applies_cloud_move_and_metadata_without_rewriting_the_text() {
    let data = tempfile::tempdir().unwrap();
    let mut session = Session::at(data.path());
    session.save("雨夜").unwrap();
    let before = session.sync_snapshot().unwrap();
    let library = session.sync_config().library_id.clone();
    let base = base_for(&plan_sync(&before, &[], &BTreeMap::new(), &library, "a", 100).manifest);
    let mut remote = before.clone();
    remote[0].path = format!("第一卷/{}", before[0].path);
    remote[0].synopsis = "第一卷摘要".to_owned();
    remote[0].pinned = true;
    let report = report(plan_sync(&before, &remote, &base, &library, "a", 101));
    assert_eq!(report.pulled.len(), 1);
    let original = session.library_root().join(&before[0].path);
    let stamp = ruwriter_core::files::stamp(&original);
    let outcome = session.apply_sync_report(&report, &before).unwrap();
    assert!(!outcome.active_changed);
    assert!(!original.exists());
    let target = session.library_root().join(&remote[0].path);
    assert_eq!(ruwriter_core::files::stamp(&target), stamp);
    let after = session.sync_snapshot().unwrap();
    assert_eq!(after[0].uid, before[0].uid);
    assert_eq!(after[0].path, remote[0].path);
    assert_eq!(after[0].synopsis, "第一卷摘要");
    assert!(after[0].pinned);
}

#[test]
fn pull_preserves_text_written_after_the_sync_started() {
    let data = tempfile::tempdir().unwrap();
    let mut session = Session::at(data.path());
    session.save("雨夜").unwrap();
    let before = session.sync_snapshot().unwrap();
    let library = session.sync_config().library_id.clone();
    let base = base_for(&plan_sync(&before, &[], &BTreeMap::new(), &library, "a", 100).manifest);
    let mut remote = before.clone();
    remote[0].content = "云端版本".to_owned();
    remote[0].hash = content_hash(&remote[0].content);
    let report = report(plan_sync(&before, &remote, &base, &library, "a", 101));
    session.autosave("雨夜\n刚写的一段").unwrap();
    let outcome = session.apply_sync_report(&report, &before).unwrap();
    assert_eq!(session.text(), "雨夜\n刚写的一段");
    assert_eq!(outcome.deferred, 1);
    assert_eq!(
        fs::read_to_string(&outcome.conflicts[0]).unwrap(),
        "云端版本"
    );
}

#[test]
fn local_edit_during_cloud_deletion_can_be_uploaded_on_retry() {
    let data = tempfile::tempdir().unwrap();
    let mut session = Session::at(data.path());
    session.save("雨夜").unwrap();
    let before = session.sync_snapshot().unwrap();
    let library = session.sync_config().library_id.clone();
    let base = base_for(&plan_sync(&before, &[], &BTreeMap::new(), &library, "a", 100).manifest);
    let remote = [deleted(&before[0].uid, &before[0].path)];
    let report = report(plan_sync(&before, &remote, &base, &library, "a", 101));
    session.autosave("雨夜\n仍在写").unwrap();
    let outcome = session.apply_sync_report(&report, &before).unwrap();
    assert_eq!(outcome.deferred, 1);

    let local = session.sync_snapshot().unwrap();
    let retry = plan_sync(
        &local,
        &remote,
        &session.sync_config().base,
        &library,
        "a",
        102,
    );
    assert_eq!(retry.uploads[0].content, "雨夜\n仍在写");
    assert!(retry.local_deletes.is_empty());
}

#[test]
fn a_missing_document_in_the_snapshot_is_an_error() {
    let data = tempfile::tempdir().unwrap();
    let mut session = Session::at(data.path());
    let id = session.active_id().unwrap();
    fs::remove_file(session.library_root().join(id)).unwrap();
    assert!(session.sync_snapshot().is_err());
}
