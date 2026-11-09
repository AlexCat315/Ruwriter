use ruwriter_core::document::{file_name_for, new_id, title_from_text};

#[test]
fn heading_becomes_title() {
    assert_eq!(
        title_from_text("# 章节标题\n\n正文", "fallback"),
        "章节标题"
    );
    assert_eq!(
        title_from_text("plain first line\nsecond", "fallback"),
        "plain first line"
    );
}

#[test]
fn blank_draft_uses_fallback() {
    assert_eq!(title_from_text("\n  \n", "未命名"), "未命名");
}

#[test]
fn long_title_truncates_on_char_boundary() {
    let long = "标题".repeat(60);
    assert_eq!(title_from_text(&long, "x").chars().count(), 48);
}

#[test]
fn file_names_never_contain_separators() {
    assert_eq!(file_name_for("a/b:c"), "a-b-c");
    assert_eq!(file_name_for(".."), "untitled");
    assert_eq!(file_name_for("  "), "untitled");
}

#[test]
fn ids_are_unique() {
    assert_ne!(new_id(), new_id());
}
