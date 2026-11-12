use ruwriter_core::files::{read_document, write_document};
use ruwriter_core::library::{Collection, Library};
use std::collections::BTreeMap;
use std::fs;

fn library_with(files: &[(&str, &str)]) -> (tempfile::TempDir, Library) {
    let root = tempfile::tempdir().expect("temp root");
    for (path, text) in files {
        write_document(&root.path().join(path), text).expect("write");
    }
    let library = Library::open(root.path(), BTreeMap::new());
    (root, library)
}

#[test]
fn scan_lists_documents_and_folders_but_not_hidden_entries() {
    let (_root, library) = library_with(&[
        ("随笔.md", "# 随笔\n\n正文"),
        ("小说/第一章.md", "雨夜"),
        ("小说/人物/林.txt", "# 林\n\n主角"),
        ("小说/图.png", "not a document"),
        (".obsidian/app.json", "{}"),
        (".draft.md", "hidden"),
    ]);
    let ids: Vec<&str> = library.documents.iter().map(|d| d.id.as_str()).collect();
    assert_eq!(ids.len(), 3, "{ids:?}");
    assert!(ids.contains(&"小说/人物/林.txt"));
    let folders: Vec<(&str, usize)> = library
        .folders
        .iter()
        .map(|f| (f.id.as_str(), f.depth))
        .collect();
    assert_eq!(folders, vec![("小说", 0), ("小说/人物", 1)]);
    assert_eq!(library.folder_count("小说"), 1);
    assert_eq!(library.folder_count(""), 1);
}

#[test]
fn titles_come_from_headings_or_file_names() {
    let (_root, library) = library_with(&[
        ("a.md", "# 标题来自正文\n\n内容"),
        ("会议记录.md", "没有标题的第一行\n第二行"),
    ]);
    let heading = library.find("a.md").unwrap();
    assert_eq!(heading.title, "标题来自正文");
    assert_eq!(heading.excerpt, "内容");
    let plain = library.find("会议记录.md").unwrap();
    assert_eq!(plain.title, "会议记录");
    assert_eq!(plain.excerpt, "没有标题的第一行 第二行");
}

#[test]
fn trash_folder_documents_are_trashed_not_folders() {
    let (_root, library) = library_with(&[("a.md", "a"), (".trash/old.md", "old")]);
    assert_eq!(library.count(Collection::Trash), 1);
    assert_eq!(library.count(Collection::All), 1);
    assert!(library.folders.is_empty());
}

#[test]
fn cache_survives_reopen_and_follows_file_changes() {
    let (root, mut library) = library_with(&[("a.md", "# 一\n\n正文")]);
    library.save_state().expect("save state");
    let reopened = Library::open(root.path(), BTreeMap::new());
    assert_eq!(reopened.find("a.md").unwrap().title, "一");

    std::thread::sleep(std::time::Duration::from_millis(20));
    write_document(&root.path().join("a.md"), "# 二\n\n更长的新正文").expect("write");
    let mut reopened = reopened;
    reopened.scan();
    assert_eq!(reopened.find("a.md").unwrap().title, "二");
}

#[test]
fn arranged_folders_keep_their_order() {
    let (_root, mut library) = library_with(&[("b/x.md", ""), ("a/x.md", ""), ("c/x.md", "")]);
    let names = |library: &Library| -> Vec<String> {
        library.folders.iter().map(|f| f.name.clone()).collect()
    };
    assert_eq!(names(&library), ["a", "b", "c"]);
    library.arrange("", "c", Some("a"));
    library.scan();
    assert_eq!(names(&library), ["c", "a", "b"]);
}

#[test]
fn writes_replace_existing_content() {
    let root = tempfile::tempdir().expect("temp root");
    let path = root.path().join("draft.md");
    write_document(&path, "old").expect("first write");
    write_document(&path, "new content").expect("second write");
    assert_eq!(read_document(&path).expect("read"), "new content");
    let leftovers: Vec<_> = fs::read_dir(root.path())
        .expect("read dir")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "temporary files must be cleaned up");
}

#[test]
fn corrupt_state_starts_fresh() {
    let root = tempfile::tempdir().expect("temp root");
    write_document(&root.path().join(".ruwriter/state.json"), "not json").expect("write");
    write_document(&root.path().join("a.md"), "a").expect("write");
    let library = Library::open(root.path(), BTreeMap::new());
    assert_eq!(library.documents.len(), 1);
}
