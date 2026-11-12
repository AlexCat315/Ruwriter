use ruwriter_core::files::{read_document, write_document};
use ruwriter_core::{AppearancePreference, Collection, Rescan, Session};
use std::path::Path;

fn library(session: &Session) -> &Path {
    session.library_root()
}

fn active_path(session: &Session) -> std::path::PathBuf {
    session.active_document().expect("active").path.clone()
}

#[test]
fn first_launch_creates_one_blank_document() {
    let root = tempfile::tempdir().expect("temp root");
    let session = Session::at(root.path());
    assert_eq!(session.documents().len(), 1);
    assert_eq!(session.text(), "");
    assert_eq!(session.title(), "未命名");
    assert!(library(&session).join("未命名.md").exists());
}

#[test]
fn autosave_persists_edits_and_save_names_the_file() {
    let root = tempfile::tempdir().expect("temp root");
    let mut session = Session::at(root.path());
    session.autosave("# 章节\n\n正文").expect("autosave");
    assert!(!session.is_dirty("# 章节\n\n正文"));
    assert_eq!(session.title(), "章节");
    session.save("# 章节\n\n正文").expect("save");
    let path = active_path(&session);
    assert_eq!(path.file_name().unwrap(), "章节.md");
    assert_eq!(read_document(&path).unwrap(), "# 章节\n\n正文");
    assert_eq!(session.active_id().as_deref(), Some("章节.md"));
}

#[test]
fn files_named_by_the_user_keep_their_names() {
    let root = tempfile::tempdir().expect("temp root");
    let mut session = Session::at(root.path());
    write_document(&library(&session).join("journal.md"), "today").expect("write");
    session.rescan();
    session.select("journal.md", "").expect("select");
    session.save("# Different heading\n\ntoday").expect("save");
    assert_eq!(session.active_id().as_deref(), Some("journal.md"));
}

#[test]
fn switching_documents_round_trips_each_text() {
    let root = tempfile::tempdir().expect("temp root");
    let mut session = Session::at(root.path());
    session.save("first draft").expect("save first");
    let first = session.active_id().expect("first id");
    assert_eq!(session.create("first draft").expect("create"), "");
    session.save("second draft").expect("save second");
    assert_eq!(
        session.select(&first, "second draft").expect("select"),
        "first draft"
    );
}

#[test]
fn new_documents_go_into_the_selected_folder() {
    let root = tempfile::tempdir().expect("temp root");
    let mut session = Session::at(root.path());
    let folder = session.create_folder("", "小说").expect("folder");
    session.set_folder(&folder);
    session.create("").expect("create");
    assert_eq!(session.active_id().as_deref(), Some("小说/未命名.md"));
    assert_eq!(session.list("").len(), 1);
}

#[test]
fn opening_an_external_file_keeps_its_path() {
    let root = tempfile::tempdir().expect("temp root");
    let outside = tempfile::tempdir().expect("outside");
    let external = outside.path().join("external.md");
    write_document(&external, "# Outside").expect("write");

    let mut session = Session::at(root.path());
    assert_eq!(session.open_path(&external, "").expect("open"), "# Outside");
    let document = session.active_document().expect("active");
    assert!(document.external);
    assert_eq!(document.path, external);

    session.save("# Outside edited").expect("save");
    assert_eq!(read_document(&external).unwrap(), "# Outside edited");
    let reopened = Session::at(root.path());
    assert!(reopened.documents().iter().any(|d| d.path == external));
}

#[test]
fn trashing_moves_files_and_restoring_puts_them_back() {
    let root = tempfile::tempdir().expect("temp root");
    let mut session = Session::at(root.path());
    let folder = session.create_folder("", "草稿").expect("folder");
    session.set_folder(&folder);
    session.create("").expect("create");
    session.save("# 一").expect("save");
    let id = session.active_id().unwrap();
    assert_eq!(id, "草稿/一.md");

    session.trash(&id, "# 一").expect("trash");
    assert!(!library(&session).join("草稿/一.md").exists());
    assert!(library(&session).join(".trash/一.md").exists());
    assert_eq!(session.count(Collection::Trash), 1);
    assert_ne!(session.active_id().as_deref(), Some(".trash/一.md"));

    let restored = session.restore(".trash/一.md").expect("restore");
    assert_eq!(restored, "草稿/一.md");
    assert!(library(&session).join("草稿/一.md").exists());
}

#[test]
fn trashing_the_last_document_creates_a_blank_one() {
    let root = tempfile::tempdir().expect("temp root");
    let mut session = Session::at(root.path());
    let only = session.active_id().unwrap();
    session.trash(&only, "").expect("trash");
    assert_eq!(session.documents().len(), 1);
    assert!(!session.active_document().unwrap().trashed);
}

#[test]
fn purge_deletes_only_from_trash() {
    let root = tempfile::tempdir().expect("temp root");
    let mut session = Session::at(root.path());
    session.save("draft").expect("save");
    let id = session.active_id().unwrap();
    assert!(session.purge(&id).is_err(), "not in trash yet");
    session.trash(&id, "draft").expect("trash");
    session.empty_trash().expect("empty");
    assert!(!library(&session).join(".trash").exists());
    assert_eq!(session.count(Collection::Trash), 0);
}

#[test]
fn pinned_documents_lead_the_list() {
    let root = tempfile::tempdir().expect("temp root");
    let mut session = Session::at(root.path());
    session.save("# old").expect("save");
    let old = session.active_id().unwrap();
    session.create("# old").expect("create");
    session.save("# new").expect("save");
    session.set_pinned(&old, true).expect("pin");
    assert_eq!(session.list("")[0].id, old);
    session.set_collection(Collection::Pinned);
    assert_eq!(session.list("").len(), 1);
    let reopened = Session::at(root.path());
    assert!(reopened.library.find(&old).unwrap().pinned);
}

#[test]
fn search_matches_title_and_excerpt() {
    let root = tempfile::tempdir().expect("temp root");
    let mut session = Session::at(root.path());
    session.save("# 海边\n\n潮水退去").expect("save");
    session.create("# 海边\n\n潮水退去").expect("create");
    session.save("# Mountains\n\nSnow line").expect("save");
    assert_eq!(session.list("潮水").len(), 1);
    assert_eq!(session.list("snow").len(), 1);
    assert_eq!(session.list("").len(), 2);
}

#[test]
fn folders_rename_move_and_carry_their_documents() {
    let root = tempfile::tempdir().expect("temp root");
    let mut session = Session::at(root.path());
    let book = session.create_folder("", "书").expect("book");
    let part = session.create_folder(&book, "第一卷").expect("part");
    session.set_folder(&part);
    session.create("").expect("create");
    session.save("# 开端").expect("save");
    assert_eq!(session.active_id().as_deref(), Some("书/第一卷/开端.md"));

    let renamed = session.rename_folder(&book, "长篇", "# 开端").expect("rename");
    assert_eq!(renamed, "长篇");
    assert_eq!(session.active_id().as_deref(), Some("长篇/第一卷/开端.md"));
    assert_eq!(session.settings().folder.as_deref(), Some("长篇/第一卷"));

    let moved = session
        .move_folder("长篇/第一卷", "", None, "# 开端")
        .expect("move");
    assert_eq!(moved, "第一卷");
    assert_eq!(session.active_id().as_deref(), Some("第一卷/开端.md"));
    assert!(
        session.move_folder("长篇", "长篇", None, "").is_err(),
        "a folder cannot contain itself"
    );
}

#[test]
fn moving_a_document_between_folders() {
    let root = tempfile::tempdir().expect("temp root");
    let mut session = Session::at(root.path());
    session.save("# 便签").expect("save");
    let notes = session.create_folder("", "笔记").expect("folder");
    session.move_document("便签.md", &notes, "# 便签").expect("move");
    assert_eq!(session.active_id().as_deref(), Some("笔记/便签.md"));
    assert!(library(&session).join("笔记/便签.md").exists());
}

#[test]
fn trashed_folders_restore_document_by_document() {
    let root = tempfile::tempdir().expect("temp root");
    let mut session = Session::at(root.path());
    let folder = session.create_folder("", "旧稿").expect("folder");
    session.set_folder(&folder);
    session.create("").expect("create");
    session.save("# 片段").expect("save");
    session.trash_folder(&folder, "# 片段").expect("trash folder");
    assert!(session.library.folders.is_empty());
    assert_eq!(session.settings().folder, None);
    assert_eq!(session.count(Collection::Trash), 1);
    let restored = session.restore(".trash/旧稿/片段.md").expect("restore");
    assert_eq!(restored, "旧稿/片段.md");
}

#[test]
fn external_edits_reload_the_open_document() {
    let root = tempfile::tempdir().expect("temp root");
    let mut session = Session::at(root.path());
    session.save("mine").expect("save");
    let path = active_path(&session);
    std::thread::sleep(std::time::Duration::from_millis(20));
    write_document(&path, "edited elsewhere, longer").expect("external write");
    assert_eq!(session.rescan(), Rescan::Reloaded);
    assert_eq!(session.text(), "edited elsewhere, longer");
    assert_eq!(session.rescan(), Rescan::Unchanged);
}

#[test]
fn a_write_after_an_external_edit_keeps_both_versions() {
    let root = tempfile::tempdir().expect("temp root");
    let mut session = Session::at(root.path());
    session.save("original").expect("save");
    let path = active_path(&session);
    std::thread::sleep(std::time::Duration::from_millis(20));
    write_document(&path, "theirs, written meanwhile").expect("external write");

    session.autosave("mine, typed meanwhile").expect("autosave");
    assert_eq!(read_document(&path).unwrap(), "theirs, written meanwhile");
    assert_eq!(session.text(), "theirs, written meanwhile");
    let copy = session.take_conflict().expect("conflict copy");
    assert_eq!(read_document(&copy).unwrap(), "mine, typed meanwhile");
}

#[test]
fn a_document_deleted_elsewhere_is_replaced() {
    let root = tempfile::tempdir().expect("temp root");
    let mut session = Session::at(root.path());
    session.save("# 会被删").expect("save");
    std::fs::remove_file(active_path(&session)).expect("delete");
    assert_eq!(session.rescan(), Rescan::Replaced);
    assert!(session.active_document().is_some());
}

#[test]
fn settings_survive_reload() {
    let root = tempfile::tempdir().expect("temp root");
    let mut session = Session::at(root.path());
    let folder = session.create_folder("", "收集").expect("folder");
    session.toggle_sidebar();
    session.toggle_preview();
    session.adjust_font_size(3);
    session.set_appearance(AppearancePreference::Dark);
    session.set_folder(&folder);
    session.set_expanded(&folder, true);
    let reloaded = Session::at(root.path());
    assert!(!reloaded.options.sidebar);
    assert!(reloaded.options.preview);
    assert_eq!(reloaded.settings().font_size, 20);
    assert_eq!(reloaded.settings().appearance, AppearancePreference::Dark);
    assert_eq!(reloaded.settings().folder.as_deref(), Some("收集"));
    assert!(reloaded.library.state.expanded.contains("收集"));
}

#[test]
fn another_library_folder_can_be_opened() {
    let root = tempfile::tempdir().expect("temp root");
    let vault = tempfile::tempdir().expect("vault");
    write_document(&vault.path().join("笔记/想法.md"), "# 想法").expect("write");
    let mut session = Session::at(root.path());
    session.open_library(vault.path(), "").expect("open library");
    assert_eq!(session.library_root(), vault.path());
    assert_eq!(session.active_id().as_deref(), Some("笔记/想法.md"));
    let reopened = Session::at(root.path());
    assert_eq!(reopened.library_root(), vault.path());
}

#[test]
fn version_one_indexes_migrate() {
    let root = tempfile::tempdir().expect("temp root");
    let documents = root.path().join("documents");
    write_document(&documents.join("第一章-1a2b3c4d.md"), "# 第一章").expect("write");
    write_document(&documents.join("旧-0000aaaa.md"), "# 旧").expect("write");
    let index = serde_json::json!({
        "documents": [
            {"id": "a", "title": "第一章", "path": documents.join("第一章-1a2b3c4d.md"),
             "external": false, "updated_at": 1, "pinned": true},
            {"id": "b", "title": "旧", "path": documents.join("旧-0000aaaa.md"),
             "external": false, "updated_at": 1, "trashed": true}
        ],
        "active": "a",
        "settings": {"font_size": 19}
    });
    write_document(&root.path().join("library.json"), &index.to_string()).expect("index");

    let mut session = Session::at(root.path());
    assert_eq!(session.active_id().as_deref(), Some("第一章-1a2b3c4d.md"));
    assert!(session.active_document().unwrap().pinned);
    assert_eq!(session.count(Collection::Trash), 1);
    assert_eq!(session.settings().font_size, 19);
    // Version 1 names were generated, so saving tidies them.
    session.save("# 第一章").expect("save");
    assert_eq!(session.active_id().as_deref(), Some("第一章.md"));
}

#[test]
fn own_writes_do_not_need_a_rescan() {
    let root = tempfile::tempdir().expect("temp root");
    let mut session = Session::at(root.path());
    session.autosave("typing").expect("autosave");
    let path = active_path(&session);
    assert!(!session.needs_rescan(std::slice::from_ref(&path)));
    let other = library(&session).join("other.md");
    write_document(&other, "x").expect("write");
    assert!(session.needs_rescan(&[path, other]));
}
