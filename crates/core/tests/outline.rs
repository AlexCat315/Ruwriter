//! Heading parsing for the outline pane.

use ruwriter_core::{Heading, heading_at, headings};

fn levels(heads: &[Heading]) -> Vec<u8> {
    heads.iter().map(|heading| heading.level).collect()
}

#[test]
fn reads_atx_headings_with_char_offsets() {
    let text = "# 一\n正文\n\n## 二\n### 三\n";
    let heads = headings(text);
    assert_eq!(levels(&heads), vec![1, 2, 3]);
    assert_eq!(heads[0].offset, 0);
    assert_eq!(heads[1].offset, 8);
    assert_eq!(heads[2].offset, 13);
    assert_eq!(heads[0].title, "一");
    assert_eq!(heads[2].title, "三");
}

#[test]
fn skips_fenced_code_blocks() {
    let text = "# 真标题\n```sh\n# 这是注释\necho hi\n```\n## 后面的标题\n~~~\n## 也是注释\n~~~\n";
    let heads = headings(text);
    assert_eq!(
        heads.iter().map(|h| h.title.as_str()).collect::<Vec<_>>(),
        vec!["真标题", "后面的标题"]
    );
}

#[test]
fn skips_indented_code_and_text_without_a_space() {
    let text = "    # 缩进代码\n#没有空格\n# 正常\n";
    let heads = headings(text);
    assert_eq!(heads.len(), 1);
    assert_eq!(heads[0].title, "正常");
}

#[test]
fn strips_closing_hashes_and_accepts_tabs() {
    let heads = headings("## 标题 ##\n###\t小标题\t###\n");
    assert_eq!(heads[0].title, "标题");
    assert_eq!(heads[1].title, "小标题");
    assert_eq!(heads[1].level, 3);
}

#[test]
fn seven_hashes_is_not_a_heading() {
    assert!(headings("####### 太深\n").is_empty());
}

#[test]
fn finds_the_section_that_holds_the_caret() {
    let heads = headings("# 一\ntext\n## 二\nmore\n");
    assert_eq!(heading_at(&heads, 0), Some(0));
    assert_eq!(heading_at(&heads, 5), Some(0));
    assert_eq!(heading_at(&heads, heads[1].offset + 2), Some(1));
}
