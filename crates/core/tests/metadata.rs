//! Author metadata (synopsis, status, tags) lives in the library state and
//! never touches the Markdown file.

use ruwriter_core::files::read_document;
use ruwriter_core::{DocStatus, Session, parse_tags};
use std::path::Path;

fn library(session: &Session) -> &Path {
    session.library_root()
}

#[test]
fn metadata_is_persisted_without_touching_the_file() {
    let root = tempfile::tempdir().expect("temp root");
    let mut session = Session::at(root.path());
    session
        .save("# 第一章\n\n雨夜里，主角推开了门。")
        .expect("save");
    let id = session.active_id().expect("active");
    let path = library(&session).join(&id);
    let before = read_document(&path).expect("read");

    session
        .set_metadata(
            &id,
            "主角第一次进入旧城，遇到反派。",
            DocStatus::Revising,
            &[
                "悬疑".to_owned(),
                "  #第一幕 ".to_owned(),
                "悬疑".to_owned(),
            ],
        )
        .expect("metadata");

    assert_eq!(read_document(&path).expect("read again"), before);
    let document = session.library.find(&id).expect("document");
    assert_eq!(document.synopsis, "主角第一次进入旧城，遇到反派。");
    assert_eq!(document.status, DocStatus::Revising);
    assert_eq!(document.tags, vec!["悬疑".to_owned(), "第一幕".to_owned()]);

    let reopened = Session::at(root.path());
    let document = reopened.library.find(&id).expect("document after reopen");
    assert_eq!(document.synopsis, "主角第一次进入旧城，遇到反派。");
    assert_eq!(document.status, DocStatus::Revising);
    assert_eq!(document.tags, vec!["悬疑".to_owned(), "第一幕".to_owned()]);
}

#[test]
fn synopsis_and_tags_are_searchable() {
    let root = tempfile::tempdir().expect("temp root");
    let mut session = Session::at(root.path());
    session.save("# 第一章\n\n正文").expect("save");
    let id = session.active_id().expect("active");
    session
        .set_metadata(
            &id,
            "主角在码头遇见旧识",
            DocStatus::Final,
            &["伏笔".to_owned()],
        )
        .expect("metadata");

    assert_eq!(session.list("码头").len(), 1);
    assert_eq!(session.list("伏笔").len(), 1);
    assert_eq!(session.list("无关").len(), 0);
}

#[test]
fn empty_synopsis_falls_back_to_the_excerpt() {
    let root = tempfile::tempdir().expect("temp root");
    let mut session = Session::at(root.path());
    session.save("# 第一章\n\n正文第一段").expect("save");
    let id = session.active_id().expect("active");
    let document = session.library.find(&id).expect("document");
    assert_eq!(document.summary(), "正文第一段");
}

#[test]
fn tags_split_on_commas_and_drop_duplicates() {
    assert_eq!(
        parse_tags("悬疑, 都市，第一幕、 #悬疑 "),
        vec!["悬疑".to_owned(), "都市".to_owned(), "第一幕".to_owned()]
    );
    assert!(parse_tags("  ,  ").is_empty());
}
