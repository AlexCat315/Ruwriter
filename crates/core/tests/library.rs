use ruwriter_core::files::{read_document, write_document};
use ruwriter_core::library::{Library, create_document, import_document};

#[test]
fn create_and_reload_index() {
    let root = tempfile::tempdir().expect("temp root");
    let mut library = Library::load(root.path());
    assert!(library.documents.is_empty());

    let document = create_document(root.path(), "# 第一章\n\n正文").expect("create");
    library.insert(document.clone());
    library.active = Some(document.id.clone());
    library.save(root.path()).expect("save index");

    let reloaded = Library::load(root.path());
    assert_eq!(reloaded, library);
    assert_eq!(
        read_document(&document.path).expect("read"),
        "# 第一章\n\n正文"
    );
    assert!(!document.external);
}

#[test]
fn import_keeps_external_path() {
    let root = tempfile::tempdir().expect("temp root");
    let external = root.path().join("notes.md");
    write_document(&external, "# 外部文件").expect("write");

    let (document, text) = import_document(&external).expect("import");
    assert!(document.external);
    assert_eq!(document.path, external);
    assert_eq!(document.title, "外部文件");
    assert_eq!(text, "# 外部文件");
}

#[test]
fn recent_orders_by_edit_time() {
    let root = tempfile::tempdir().expect("temp root");
    let mut library = Library::default();
    let first = create_document(root.path(), "first").expect("create");
    let mut second = create_document(root.path(), "second").expect("create");
    second.updated_at = first.updated_at + 10;
    library.insert(first.clone());
    library.insert(second.clone());
    assert_eq!(library.recent()[0].id, second.id);
}

#[test]
fn inserting_same_path_replaces_entry() {
    let root = tempfile::tempdir().expect("temp root");
    let mut library = Library::default();
    let document = create_document(root.path(), "text").expect("create");
    library.insert(document.clone());
    library.insert(document.clone());
    assert_eq!(library.documents.len(), 1);
    assert_eq!(
        library.remove(&document.id).map(|value| value.id),
        Some(document.id)
    );
    assert!(library.documents.is_empty());
}

#[test]
fn writes_replace_existing_content() {
    let root = tempfile::tempdir().expect("temp root");
    let path = root.path().join("draft.md");
    write_document(&path, "old").expect("first write");
    write_document(&path, "new content").expect("second write");
    assert_eq!(read_document(&path).expect("read"), "new content");
    let leftovers: Vec<_> = std::fs::read_dir(root.path())
        .expect("read dir")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "temporary files must be cleaned up");
}

#[test]
fn corrupt_index_starts_empty() {
    let root = tempfile::tempdir().expect("temp root");
    write_document(&root.path().join("library.json"), "not json").expect("write");
    assert_eq!(Library::load(root.path()), Library::default());
}
