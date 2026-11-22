use ruwriter_core::files::{read_document, write_document};
use ruwriter_core::{
    AiPreferences, AppearancePreference, Collection, Rescan, Session, WritingBrief, WritingMode,
};
use std::path::Path;

fn library(session: &Session) -> &Path {
    session.library_root()
}

fn active_path(session: &Session) -> std::path::PathBuf {
    session.active_document().expect("active").path.clone()
}

#[test]
fn active_file_format_overrides_new_document_preference() {
    let root = tempfile::tempdir().unwrap();
    let mut session = Session::at(root.path());
    assert_eq!(session.writing_mode(), WritingMode::Novel);
    session.set_writing_mode(WritingMode::Markdown);
    assert_eq!(session.writing_mode(), WritingMode::Novel);
    session.create("").unwrap();
    assert_eq!(session.writing_mode(), WritingMode::Markdown);
    let outside = root.path().join("正文.TXT");
    std::fs::write(&outside, "# 字面井号").unwrap();
    session.open_path(&outside, "").unwrap();
    assert_eq!(session.writing_mode(), WritingMode::Novel);
}

#[test]
fn batch_import_copies_legacy_text_and_preserves_names_and_originals() {
    let root = tempfile::tempdir().unwrap();
    let mut session = Session::at(root.path());
    let folder = session.create_folder("", "小说").unwrap();
    let source = root.path().join("章节.TXT");
    let markdown = root.path().join("提纲.markdown");
    let unsupported = root.path().join("附件.pdf");
    let gbk = [0xd3, 0xea, 0xd2, 0xb9];
    std::fs::write(&source, gbk).unwrap();
    let utf16: Vec<_> = [0xff, 0xfe]
        .into_iter()
        .chain("# 提纲".encode_utf16().flat_map(u16::to_le_bytes))
        .collect();
    std::fs::write(&markdown, &utf16).unwrap();
    let (copied, failures) = session
        .import_files(
            &[
                source.clone(),
                source.clone(),
                unsupported,
                markdown.clone(),
            ],
            &folder,
            "",
        )
        .unwrap();
    assert_eq!(copied, 3);
    assert_eq!(failures.len(), 1);
    let target = session.library_root().join(folder);
    assert_eq!(
        std::fs::read_to_string(target.join("章节.TXT")).unwrap(),
        "雨夜"
    );
    assert_eq!(
        std::fs::read_to_string(target.join("章节 2.TXT")).unwrap(),
        "雨夜"
    );
    assert_eq!(
        std::fs::read_to_string(target.join("提纲.markdown")).unwrap(),
        "# 提纲"
    );
    assert_eq!(std::fs::read(source).unwrap(), gbk);
    assert_eq!(std::fs::read(markdown).unwrap(), utf16);
    assert_eq!(session.writing_mode(), WritingMode::Markdown);
}

#[test]
fn a_saved_single_ai_model_migrates_and_parallel_selection_survives_relaunch() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("library.json"),
        r#"{"version":2,"ai":{"endpoint":"http://127.0.0.1:1234/v1","model":"draft","tools":false}}"#,
    )
    .unwrap();
    let mut session = Session::at(root.path());
    let mut settings = session.ai_preferences().clone();
    assert_eq!(settings.selected, vec![0]);
    assert_eq!(settings.models[0].model, "draft");
    assert!(!settings.models[0].tools);
    settings.models.push(AiPreferences {
        model: "review".to_owned(),
        ..Default::default()
    });
    settings.selected = vec![1, 42, 0, 1];
    session.set_ai_preferences(settings).unwrap();
    let reopened = Session::at(root.path());
    let saved = reopened.ai_preferences();
    assert_eq!(saved.selected, vec![1, 0]);
    assert_eq!(saved.models[1].model, "review");
    assert_eq!(saved.models[0].endpoint, "http://127.0.0.1:1234/v1");
}

#[test]
fn a_work_brief_survives_folder_rename_and_reopening() {
    let root = tempfile::tempdir().unwrap();
    let mut session = Session::at(root.path());
    let work = session.create_folder("", "旧书名").unwrap();
    let brief = WritingBrief {
        canon: "钥匙只能开北门".to_owned(),
        voice: "短句，限制于人物视角".to_owned(),
        avoid: "命运的齿轮".to_owned(),
        voice_samples: vec!["她说完，顺手把炉门关上。".to_owned()],
        story: ruwriter_core::StoryProfile {
            outline: "守门人交出北门钥匙。".to_owned(),
            characters: "阿玉，守门人。".to_owned(),
            open_threads: "北门还没开。".to_owned(),
            ..Default::default()
        },
        ..Default::default()
    };
    session
        .library
        .save_writing_brief(&work, brief.clone())
        .unwrap();
    session.rename_folder(&work, "新书名", "").unwrap();
    let reopened = Session::at(root.path());
    assert_eq!(
        reopened.library.state.writing_briefs.get("新书名"),
        Some(&brief)
    );
    assert!(!reopened.library.state.writing_briefs.contains_key("旧书名"));
}

#[test]
fn first_launch_creates_one_blank_document() {
    let root = tempfile::tempdir().expect("temp root");
    let session = Session::at(root.path());
    assert_eq!(session.documents().len(), 1);
    assert_eq!(session.text(), "");
    assert_eq!(session.title(), "未命名");
    assert!(library(&session).join("未命名.txt").exists());
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
    assert_eq!(path.file_name().unwrap(), "章节.txt");
    assert_eq!(read_document(&path).unwrap(), "# 章节\n\n正文");
    assert_eq!(session.active_id().as_deref(), Some("章节.txt"));
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
fn caret_position_survives_switching_and_reload() {
    let root = tempfile::tempdir().expect("temp root");
    let mut session = Session::at(root.path());
    session.save("第一行\n第二行").expect("save first");
    let first = session.active_id().expect("first id");
    session.remember_caret(4);
    session.persist_caret();
    assert_eq!(session.active_caret(), 4);

    session.create("第一行\n第二行").expect("create second");
    session.save("另一个文稿").expect("save second");
    session
        .select(&first, "另一个文稿")
        .expect("switch back to first");
    assert_eq!(session.active_caret(), 4);

    let reloaded = Session::at(root.path());
    assert_eq!(reloaded.active_caret(), 4);
}

#[test]
fn new_documents_go_into_the_selected_folder() {
    let root = tempfile::tempdir().expect("temp root");
    let mut session = Session::at(root.path());
    let folder = session.create_folder("", "小说").expect("folder");
    session.set_folder(&folder);
    session.create("").expect("create");
    assert_eq!(session.active_id().as_deref(), Some("小说/未命名.txt"));
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
    assert_eq!(id, "草稿/一.txt");

    session.trash(&id, "# 一").expect("trash");
    assert!(!library(&session).join("草稿/一.txt").exists());
    assert!(library(&session).join(".trash/一.txt").exists());
    assert_eq!(session.count(Collection::Trash), 1);
    assert_ne!(session.active_id().as_deref(), Some(".trash/一.txt"));

    let restored = session.restore(".trash/一.txt").expect("restore");
    assert_eq!(restored, "草稿/一.txt");
    assert!(library(&session).join("草稿/一.txt").exists());
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
    assert_eq!(session.active_id().as_deref(), Some("书/第一卷/开端.txt"));

    let renamed = session
        .rename_folder(&book, "长篇", "# 开端")
        .expect("rename");
    assert_eq!(renamed, "长篇");
    assert_eq!(session.active_id().as_deref(), Some("长篇/第一卷/开端.txt"));
    assert_eq!(session.settings().folder.as_deref(), Some("长篇/第一卷"));

    let moved = session
        .move_folder("长篇/第一卷", "", None, "# 开端")
        .expect("move");
    assert_eq!(moved, "第一卷");
    assert_eq!(session.active_id().as_deref(), Some("第一卷/开端.txt"));
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
    session
        .move_document("便签.txt", &notes, "# 便签")
        .expect("move");
    assert_eq!(session.active_id().as_deref(), Some("笔记/便签.txt"));
    assert!(library(&session).join("笔记/便签.txt").exists());
}

#[test]
fn trashed_folders_restore_document_by_document() {
    let root = tempfile::tempdir().expect("temp root");
    let mut session = Session::at(root.path());
    let folder = session.create_folder("", "旧稿").expect("folder");
    session.set_folder(&folder);
    session.create("").expect("create");
    session.save("# 片段").expect("save");
    session
        .trash_folder(&folder, "# 片段")
        .expect("trash folder");
    assert!(session.library.folders.is_empty());
    assert_eq!(session.settings().folder, None);
    assert_eq!(session.count(Collection::Trash), 1);
    let restored = session.restore(".trash/旧稿/片段.txt").expect("restore");
    assert_eq!(restored, "旧稿/片段.txt");
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
    session
        .open_library(vault.path(), "")
        .expect("open library");
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

#[test]
fn sort_choice_orders_rows_and_survives_reload() {
    use ruwriter_core::SortOrder;

    let root = tempfile::tempdir().expect("temp root");
    let mut session = Session::at(root.path());
    session.save("# 丙\n\n一二三四五").expect("save 丙");
    session.create("# 丙\n\n一二三四五").expect("create");
    session.save("# 甲\n\n一二三四五六七八").expect("save 甲");
    session.create("# 甲\n\n一二三四五六七八").expect("create");
    session.save("# 乙\n\n一二三").expect("save 乙");

    session.set_sort(SortOrder::Title);
    let titles: Vec<String> = session.list("").into_iter().map(|d| d.title).collect();
    assert_eq!(titles, ["丙", "乙", "甲"]);

    session.set_sort(SortOrder::Words);
    let titles: Vec<String> = session.list("").into_iter().map(|d| d.title).collect();
    assert_eq!(titles, ["甲", "丙", "乙"]);

    session.set_pinned("乙.txt", true).expect("pin");
    let titles: Vec<String> = session.list("").into_iter().map(|d| d.title).collect();
    assert_eq!(titles, ["乙", "甲", "丙"]);

    let reopened = Session::at(root.path());
    assert_eq!(reopened.settings().sort, SortOrder::Words);
    let titles: Vec<String> = reopened.list("").into_iter().map(|d| d.title).collect();
    assert_eq!(titles, ["乙", "甲", "丙"]);
}

#[test]
fn snapshots_capture_before_restore_and_survive_a_rename() {
    let root = tempfile::tempdir().expect("temp root");
    let mut session = Session::at(root.path());
    session.autosave("第一版").expect("autosave");
    let before = session.snapshots();
    assert!(!before.is_empty(), "a snapshot is taken while writing");

    session.autosave("第二版").expect("autosave");
    let name = before[0].name.clone();
    let restored = session.restore_snapshot(&name, "第二版").expect("restore");
    assert_eq!(restored, "第一版");
    assert_eq!(session.text(), "第一版");
    assert_eq!(read_document(&active_path(&session)).unwrap(), "第一版");
    assert!(
        session
            .snapshots()
            .iter()
            .any(|snapshot| snapshot.excerpt.contains("第二版")),
        "the replaced text is kept as a snapshot"
    );

    session.save("第一版").expect("save");
    let renamed = session.active_id().unwrap();
    assert!(renamed.ends_with("第一版.txt"));
    assert!(
        !session.snapshots().is_empty(),
        "snapshots follow the document after its file is renamed"
    );
}
