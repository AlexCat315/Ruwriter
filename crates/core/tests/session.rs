use ruwriter_core::files::{read_document, write_document};
use ruwriter_core::{AppearancePreference, Collection, Session};

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

#[test]
fn trashing_the_open_document_opens_a_neighbour() {
    let root = tempfile::tempdir().expect("temp root");
    let mut session = Session::at(root.path());
    session.save("# 一").expect("save");
    let first = session.active_id().unwrap();
    session.create("# 一").expect("create");
    session.save("# 二").expect("save");
    let second = session.active_id().unwrap();

    session.trash(&second, "# 二").expect("trash");
    assert_eq!(session.active_id().as_deref(), Some(first.as_str()));
    assert_eq!(session.text(), "# 一");
    assert_eq!(session.documents().len(), 1);
    assert_eq!(session.count(Collection::Trash), 1);

    session.restore(&second).expect("restore");
    assert_eq!(session.documents().len(), 2);
}

#[test]
fn trashing_the_last_document_creates_a_blank_one() {
    let root = tempfile::tempdir().expect("temp root");
    let mut session = Session::at(root.path());
    let only = session.active_id().unwrap();
    session.trash(&only, "").expect("trash");
    assert_ne!(session.active_id(), Some(only));
    assert_eq!(session.documents().len(), 1);
}

#[test]
fn purge_deletes_library_files_only_from_trash() {
    let root = tempfile::tempdir().expect("temp root");
    let mut session = Session::at(root.path());
    session.save("draft").expect("save");
    let id = session.active_id().unwrap();
    let path = session.active_document().unwrap().path.clone();
    assert!(session.purge(&id).is_err(), "not in trash yet");

    session.trash(&id, "draft").expect("trash");
    session.empty_trash().expect("empty");
    assert!(!path.exists());
    assert!(session.library.find(&id).is_none());
}

#[test]
fn pinned_documents_lead_the_list() {
    let root = tempfile::tempdir().expect("temp root");
    let mut session = Session::at(root.path());
    session.save("# old").expect("save");
    let old = session.active_id().unwrap();
    session.create("# old").expect("create");
    session.save("# new").expect("save");

    assert_ne!(session.list("")[0].id, old);
    session.set_pinned(&old, true).expect("pin");
    assert_eq!(session.list("")[0].id, old);
    session.set_collection(Collection::Pinned);
    assert_eq!(session.list("").len(), 1);
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
fn settings_survive_reload() {
    let root = tempfile::tempdir().expect("temp root");
    let mut session = Session::at(root.path());
    session.toggle_sidebar();
    session.toggle_preview();
    session.adjust_font_size(3);
    session.set_appearance(AppearancePreference::Dark);
    let reloaded = Session::at(root.path());
    assert!(!reloaded.options.sidebar);
    assert!(reloaded.options.preview);
    assert_eq!(reloaded.settings().font_size, 20);
    assert_eq!(reloaded.settings().appearance, AppearancePreference::Dark);
}

#[test]
fn reopening_a_library_file_selects_it() {
    let root = tempfile::tempdir().expect("temp root");
    let mut session = Session::at(root.path());
    session.save("same").expect("save");
    let id = session.active_id().unwrap();
    let path = session.active_document().unwrap().path.clone();
    session.create("same").expect("create");
    session.open_path(&path, "").expect("open");
    assert_eq!(session.active_id(), Some(id));
    assert_eq!(session.documents().len(), 2);
}

#[test]
fn saving_names_library_files_after_their_title() {
    let root = tempfile::tempdir().expect("temp root");
    let mut session = Session::at(root.path());
    session.save("# 海边的雨\n\n正文").expect("save");
    let path = session.active_document().unwrap().path.clone();
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        name.starts_with("海边的雨-") && name.ends_with(".md"),
        "{name}"
    );
    assert_eq!(read_document(&path).unwrap(), "# 海边的雨\n\n正文");
    let reloaded = Session::at(root.path());
    assert_eq!(reloaded.text(), "# 海边的雨\n\n正文");
}
