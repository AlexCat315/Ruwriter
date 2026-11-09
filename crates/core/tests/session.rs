use ruwriter_core::Session;
use ruwriter_core::files::{read_document, write_document};

#[test]
fn first_launch_creates_one_blank_document() {
    let root = tempfile::tempdir().expect("temp root");
    let session = Session::at(root.path());
    assert_eq!(session.documents().len(), 1);
    assert_eq!(session.text(), "");
    assert_eq!(session.title(), "未命名");
}

#[test]
fn autosave_persists_edits() {
    let root = tempfile::tempdir().expect("temp root");
    let mut session = Session::at(root.path());
    session.autosave("# 章节\n\n正文").expect("autosave");
    assert!(!session.is_dirty("# 章节\n\n正文"));
    assert_eq!(
        read_document(&session.active_document().unwrap().path).unwrap(),
        "# 章节\n\n正文"
    );
    assert_eq!(session.title(), "章节");
}

#[test]
fn switching_documents_round_trips_each_text() {
    let root = tempfile::tempdir().expect("temp root");
    let mut session = Session::at(root.path());
    let first = session.active_id().expect("first id");
    session.save("first draft").expect("save first");
    let second = session.create("first draft").expect("create").to_owned();
    assert_eq!(second, "");

    session.save("second draft").expect("save second");
    assert_eq!(
        session.select(&first, "second draft").expect("select"),
        "first draft"
    );
    assert_eq!(session.text(), "first draft");
}

#[test]
fn opening_an_external_file_keeps_its_path() {
    let root = tempfile::tempdir().expect("temp root");
    let external = root.path().join("external.md");
    write_document(&external, "# Outside").expect("write");

    let mut session = Session::at(root.path());
    assert_eq!(session.open_path(&external, "").expect("open"), "# Outside");
    let document = session.active_document().expect("active");
    assert!(document.external);
    assert_eq!(document.path, external);

    session.save("# Outside edited").expect("save");
    assert_eq!(read_document(&external).unwrap(), "# Outside edited");
}

#[test]
fn preview_and_focus_toggle_independently() {
    let root = tempfile::tempdir().expect("temp root");
    let mut session = Session::at(root.path());
    session.toggle_preview();
    assert!(session.options.preview);
    assert!(!session.options.focus);
    session.toggle_focus();
    assert!(session.options.preview && session.options.focus);
}
